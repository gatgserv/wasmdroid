package dev.wasmdroid.app.ui

import kotlinx.coroutines.*
import kotlinx.coroutines.flow.*
import org.json.JSONObject

internal enum class StackEditProblem { Unavailable, Changed, Busy, Unsupported, Invalid, Failed, Large }
internal data class StackEditState(
    val open:Boolean=false,val id:String?=null,val version:ULong?=null,
    val document:String="",val original:String="",val preview:String?=null,
    val requirements:String?=null,val epoch:Long=0,val revision:Long=0,val loading:Boolean=false,
    val reviewing:Boolean=false,val applying:Boolean=false,val applied:Boolean=false,
    val problem:StackEditProblem?=null,val pendingId:String?=null,
){
    val busy get()=loading||reviewing||applying
    val dirty get()=document!=original
    override fun toString()="StackEditState(private)"
}
internal interface StackEditBackend {
    val current:Boolean
    suspend fun source(id:String):String
    suspend fun requirements(document:String):String
    suspend fun preview(id:String,version:ULong,document:String):String
    suspend fun apply(id:String,preview:String):String
}

/** Private bounded ViewModel draft. Never place source, specs or env in bundles. */
internal class StackEditController(
    private val scope:CoroutineScope,private val capture:suspend()->StackEditBackend,
    private val changed:suspend(StackEditBackend)->Unit,
){
    private val mutable=MutableStateFlow(StackEditState())
    val state:StateFlow<StackEditState> = mutable.asStateFlow()
    private var job:Job?=null
    private var backend:StackEditBackend?=null
    private var serial=0L
    fun open(id:String){
        val old=mutable.value
        if(old.applying)return
        if(old.id==id&&old.version!=null&&!old.applied){
            job?.cancel();mutable.update{it.copy(open=true,epoch=++serial,revision=serial,reviewing=false,preview=null,pendingId=null)};return
        }
        if(old.dirty&&old.id!=id){
            job?.cancel();mutable.update{it.copy(open=true,pendingId=id,revision=++serial,reviewing=false,preview=null)};return
        }
        load(id)
    }
    fun acceptSwitch(epoch:Long,id:String){val old=mutable.value;if(old.open&&old.epoch==epoch&&!old.busy&&old.pendingId==id)load(id)}
    fun cancelSwitch(epoch:Long){if(mutable.value.epoch==epoch)mutable.update{it.copy(pendingId=null)}}
    fun reload(epoch:Long){val old=mutable.value;if(old.open&&old.epoch==epoch&&!old.busy)old.id?.let(::load)}
    private fun load(id:String){
        job?.cancel();backend=null
        val revision=++serial
        mutable.value=StackEditState(open=true,id=id,epoch=revision,revision=revision,loading=true)
        job=scope.launch{
            try{
                val connection=capture();check(connection.current){"connection changed"}
                val record=JSONObject(connection.source(id))
                val document=record.getString("document")
                check(record.getString("id")==id&&record.getLong("version")>=0){"definition changed"}
                if(document.toByteArray(Charsets.UTF_8).size>MAX_STACK_DOCUMENT_BYTES)throw StackDocumentTooLarge()
                if(current(revision)&&connection.current){
                    backend=connection
                    mutable.update{it.copy(version=record.getLong("version").toULong(),document=document,original=document,loading=false)}
                }else if(current(revision))fail(revision,StackEditProblem.Unavailable)
            }catch(failure:Exception){if(failure is CancellationException)throw failure;fail(revision,classify(failure))}
        }
    }
    fun edit(epoch:Long,document:String){
        val old=mutable.value
        if(!old.open||old.epoch!=epoch||old.loading||old.applying||old.applied||old.pendingId!=null)return
        job?.cancel()
        val revision=++serial
        if(document.toByteArray(Charsets.UTF_8).size>MAX_STACK_DOCUMENT_BYTES){
            mutable.value=old.copy(revision=revision,preview=null,requirements=null,reviewing=false,problem=StackEditProblem.Large);return
        }
        mutable.value=old.copy(document=document,revision=revision,preview=null,requirements=null,reviewing=false,problem=null,applied=false)
    }
    fun review(epoch:Long,revision:Long){
        val old=mutable.value;val connection=backend
        if(!old.open||old.epoch!=epoch||old.revision!=revision||old.busy||old.applied||old.pendingId!=null||old.id==null||old.version==null||old.document.isBlank())return
        if(connection?.current!=true){fail(old.revision,StackEditProblem.Unavailable);return}
        mutable.update{it.copy(reviewing=true,preview=null,requirements=null,problem=null)}
        job=scope.launch{
            try{
                val requirements=connection.requirements(old.document)
                if(!current(old.revision))return@launch
                mutable.update{it.copy(requirements=requirements)}
                val preview=connection.preview(old.id,old.version,old.document)
                if(current(old.revision)&&connection.current)mutable.update{it.copy(reviewing=false,preview=preview)}
                else if(current(old.revision))fail(old.revision,StackEditProblem.Unavailable)
            }catch(failure:Exception){if(failure is CancellationException)throw failure;fail(old.revision,classify(failure))}
        }
    }
    fun apply(epoch:Long,preview:String){
        val old=mutable.value;val connection=backend
        if(!old.open||old.epoch!=epoch||old.busy||old.pendingId!=null||old.id==null||old.preview==null||old.preview!=preview)return
        if(connection?.current!=true){fail(old.revision,StackEditProblem.Unavailable);return}
        mutable.update{it.copy(applying=true,problem=null)}
        job=scope.launch{
            try{
                connection.apply(old.id,old.preview)
                // Durable owner acknowledgement wins over a later UI disconnect.
                if(current(old.revision)){
                    backend=null
                    mutable.value=StackEditState(open=mutable.value.open,id=old.id,epoch=old.epoch,revision=++serial,applied=true)
                }
                try{changed(connection)}catch(failure:Exception){if(failure is CancellationException)throw failure}
            }catch(failure:Exception){if(failure is CancellationException)throw failure;fail(old.revision,classify(failure))}
        }
    }
    fun leave(epoch:Long){
        if(mutable.value.epoch!=epoch)return
        if(mutable.value.applying){mutable.update{it.copy(open=false)};return}
        job?.cancel();mutable.update{it.copy(open=false,loading=false,reviewing=false,preview=null,pendingId=null,revision=++serial)}
    }
    fun discard(epoch:Long){if(mutable.value.epoch==epoch&&!mutable.value.applying){job?.cancel();backend=null;mutable.value=StackEditState(revision=++serial)}}
    fun close(){job?.cancel();backend=null;mutable.value=StackEditState(revision=++serial)}
    private fun current(revision:Long)=mutable.value.revision==revision
    private fun fail(revision:Long,problem:StackEditProblem){if(current(revision))mutable.update{it.copy(loading=false,reviewing=false,applying=false,preview=null,problem=problem)}}
    private fun classify(failure:Throwable):StackEditProblem{
        if(failure is StackDocumentTooLarge)return StackEditProblem.Large
        var actual=failure;repeat(8){actual.cause?.let{actual=it}}
        val text=actual.message.orEmpty()
        return when{
            text.contains("stale",true)||text.contains("changed",true)->StackEditProblem.Changed
            text.contains("connection",true)||text.contains("closing",true)->StackEditProblem.Unavailable
            text.contains("stop",true)||text.contains("active",true)||text.contains("lease",true)->StackEditProblem.Busy
            text.contains("unsupported",true)->StackEditProblem.Unsupported
            text.contains("invalid",true)||text.contains("not found",true)||text.contains("missing",true)->StackEditProblem.Invalid
            else->StackEditProblem.Failed
        }
    }
}
