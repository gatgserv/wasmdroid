package dev.wasmdroid.app.ui

import kotlinx.coroutines.*
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.*
import org.json.JSONObject

internal data class TerminalCell(val text:String,val foreground:Int,val background:Int,val attributes:Int,val width:Int)
internal data class TerminalFrame(val token:Long,val version:Long,val rows:Int,val cols:Int,val cells:List<List<TerminalCell>>,
    val cursorRow:Int,val cursorCol:Int,val hideCursor:Boolean,val applicationCursor:Boolean,val bracketedPaste:Boolean,
    val scrollback:Int,val scrollbackLength:Int,val discarded:Boolean,val replyOverflow:Boolean){
    fun plainText()=cells.joinToString("\n"){row->row.joinToString(""){cell->if(cell.width==0)"" else cell.text.ifEmpty{" "}}.trimEnd()}
    companion object {
        fun decode(pages:List<JSONObject>):TerminalFrame{
            require(pages.isNotEmpty()&&pages.size<=8){"Invalid terminal page count"}
            val first=pages.first();val rows=first.getInt("rows");val cols=first.getInt("cols");val token=first.getLong("token");val version=first.getLong("version")
            require(rows in 1..128&&cols in 1..256&&token>0&&version>=0){"Invalid terminal dimensions"}
            val cells=mutableListOf<List<TerminalCell>>()
            for(page in pages){
                require(page.getLong("token")==token&&page.getLong("version")==version&&page.getInt("rows")==rows&&page.getInt("cols")==cols&&page.getInt("start_row")==cells.size){"Terminal snapshot changed"}
                val lines=page.getJSONArray("cells");require(lines.length() in 1..16){"Invalid terminal page rows"}
                for(index in 0 until lines.length()){
                    val line=lines.getJSONArray(index);require(line.length()==cols){"Invalid terminal row"}
                    cells+=List(cols){col->val value=line.getJSONArray(col);require(value.length()==5){"Invalid terminal cell"}
                        TerminalCell(value.getString(0),value.getInt(1),value.getInt(2),value.getInt(3),value.getInt(4)).also{require(it.text.toByteArray(Charsets.UTF_8).size<=22&&it.attributes in 0..31&&it.width in 0..2){"Terminal cell exceeds limits"}}
                    }
                }
                require(cells.size<=rows){"Too many terminal rows"}
            }
            require(cells.size==rows&&!pages.last().getBoolean("has_more")){"Incomplete terminal snapshot"}
            return TerminalFrame(token,version,rows,cols,cells,first.getInt("cursor_row"),first.getInt("cursor_col"),first.getBoolean("hide_cursor"),first.getBoolean("application_cursor"),first.getBoolean("bracketed_paste"),first.getInt("scrollback"),first.getInt("scrollback_len"),first.getBoolean("history_discarded"),first.getBoolean("reply_overflow"))
        }
    }
}
internal data class TerminalUiState(val frame:TerminalFrame?=null,val generation:String?=null,val live:Boolean=false,val inputOpen:Boolean=false,val error:String?=null)
internal data class TerminalRect(val cols:Int,val rows:Int,val width:Int,val height:Int)

/** One screen attachment keeps its exact worker generation. Closing the screen
 * discards unsent keystrokes and leaves the running guest/foreground owner alone. */
internal class TerminalController(private val model:AppModel,private val id:String,parent:CoroutineScope,
    private val restoredGeneration:String?,private val onGeneration:(String)->Unit){
    private val job=SupervisorJob(parent.coroutineContext[Job]);private val scope=CoroutineScope(parent.coroutineContext+job)
    private val mutable=MutableStateFlow(TerminalUiState());val state=mutable.asStateFlow()
    private val attached=CompletableDeferred<String>()
    private val input=Channel<ByteArray>(16);private val rects=Channel<TerminalRect>(Channel.CONFLATED)
    private val scroll=MutableStateFlow(0)
    init{
        scope.launch{attachAndFollow()}
        scope.launch{
            val generation=runCatching{attached.await()}.getOrNull()?:return@launch
            for(bytes in input){
                try{
                    check(mutable.value.live&&mutable.value.inputOpen){"Terminal input ended"}
                    for(start in bytes.indices step 4000)model.writeStdin(id,generation,bytes.copyOfRange(start,minOf(start+4000,bytes.size)))
                }catch(failure:Exception){if(failure is CancellationException)throw failure;while(input.tryReceive().isSuccess){};fail("Terminal input failed; queued input was discarded")}
            }
        }
        scope.launch{
            val generation=runCatching{attached.await()}.getOrNull()?:return@launch
            var previous:TerminalRect?=null
            for(rect in rects){if(rect==previous||!mutable.value.live)continue
                try{model.resizeTerminal(id,generation,rect.cols,rect.rows,rect.width,rect.height);previous=rect}
                catch(failure:Exception){if(failure is CancellationException)throw failure;fail("Unable to resize this terminal")}
            }
        }
    }
    private suspend fun attachAndFollow(){
        try{
            val current=currentSession()
            val generation=restoredGeneration?:current?.getString("generation")?:model.logs(id,"{\"tail\":1}")?.let{JSONObject(it).getString("generation")}?:error("No terminal session exists yet")
            onGeneration(generation)
            val live=current?.optString("generation")==generation
            val inputOpen=live&&(current?.optJSONObject("runtime_info")?.optBoolean("stdin_open")==true)
            mutable.value=TerminalUiState(generation=generation,live=live,inputOpen=inputOpen);attached.complete(generation)
            do{
                try{
                    val frame=readFrame(generation,mutable.value.live,scroll.value)
                    if(frame!=null)mutable.update{it.copy(frame=frame,error=null)}
                }catch(failure:Exception){
                    if(failure is CancellationException)throw failure
                    val active=currentSession()?.optString("generation")==generation
                    if(!active){mutable.update{it.copy(live=false,inputOpen=false)};readFrame(generation,false,0)?.let{frame->mutable.update{it.copy(frame=frame,error=null)}}}
                    else fail("Unable to refresh terminal display")
                }
                if(mutable.value.live)delay(150)
            }while(mutable.value.live)
        }catch(failure:Exception){if(failure is CancellationException)throw failure;attached.completeExceptionally(failure);fail("The terminal could not be attached")}
    }
    private suspend fun readFrame(generation:String,live:Boolean,history:Int):TerminalFrame?{
        repeat(3){attempt->
            try{
                val pages=mutableListOf<JSONObject>();var row=0;var token:Long?=null
                do{
                    val raw=if(live)model.terminalFrame(id,generation,JSONObject().put("scrollback",history).put("start_row",row).also{query->
                        token?.let{query.put("token",it)}
                        if(row==0&&mutable.value.frame?.scrollback==history)mutable.value.frame?.let{query.put("known_version",it.version)}
                    }.toString())
                        else model.savedTerminal(id,generation,row)?:return null
                    require(raw.length<=256*1024){"Terminal page exceeds limit"}
                    val page=withContext(Dispatchers.Default){JSONObject(raw)}
                    if(row==0&&page.optBoolean("unchanged")){return mutable.value.frame?.takeIf{it.version==page.getLong("version")}?:error("Terminal cache is unavailable")}
                    pages+=page;require(pages.size<=8){"Too many terminal pages"}
                    token=page.getLong("token");row+=page.getJSONArray("cells").length()
                }while(page.getBoolean("has_more"))
                return withContext(Dispatchers.Default){TerminalFrame.decode(pages)}
            }catch(failure:Exception){if(failure is CancellationException)throw failure;if(attempt==2)throw failure;delay(40)}
        }
        return null
    }
    private suspend fun currentSession():JSONObject?=try{JSONObject(model.terminalSession(id))}catch(failure:Exception){if(failure is CancellationException)throw failure;null}
    fun send(bytes:ByteArray){
        if(bytes.isEmpty())return
        if(!mutable.value.live||!mutable.value.inputOpen){fail("Terminal input is closed");return}
        if(bytes.size>64*1024||input.trySend(bytes.copyOf()).isFailure)fail("Terminal input queue is full")
    }
    fun paste(text:String){
        if(text.any{it.code<32&&it !in "\n\r\t"}){fail("Paste contains terminal control characters");return}
        val bytes=text.toByteArray(Charsets.UTF_8)
        if(bytes.size>60*1024){fail("Paste exceeds 60 KiB limit");return}
        if(mutable.value.frame?.bracketedPaste==true)send("\u001b[200~".toByteArray()+bytes+"\u001b[201~".toByteArray())else send(bytes)
    }
    fun resize(rect:TerminalRect){rects.trySend(rect)}
    fun scrollBy(rows:Int){val maximum=mutable.value.frame?.scrollbackLength?:0;scroll.update{(it+rows).coerceIn(0,maximum)}}
    fun liveView(){scroll.value=0}
    fun interrupt(){scope.launch{try{model.interrupt(id,attached.await())}catch(failure:Exception){if(failure is CancellationException)throw failure;fail("Guest interrupt could not be delivered")}}}
    fun eof(){scope.launch{try{model.closeStdin(id,attached.await());mutable.update{it.copy(inputOpen=false)}}catch(failure:Exception){if(failure is CancellationException)throw failure;fail("Input could not be closed")}}}
    fun dismissError(){mutable.update{it.copy(error=null)}}
    fun inputError(message:String){fail(message)}
    private fun fail(message:String){mutable.update{it.copy(error=message)}}
    fun close(){input.close();rects.close();job.cancel()}
}
