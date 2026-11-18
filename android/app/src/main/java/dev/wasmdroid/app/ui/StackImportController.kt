package dev.wasmdroid.app.ui

import android.app.Application
import android.net.Uri
import dev.wasmdroid.app.R
import dev.wasmdroid.engine.DataTransferControl
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.*
import java.io.ByteArrayOutputStream
import java.nio.ByteBuffer
import java.nio.charset.CodingErrorAction

/** Private in-memory draft. Never save document/env text in an Android Bundle. */
data class StackImportState(
    val id:String="", val document:String="", val revision:Long=0,
    val preview:String?=null, val requirements:String?=null,val busy:Boolean=false, val error:Int?=null,
    val detail:String?=null, val createdId:String?=null,
){override fun toString()="StackImportState(private)"}
internal class StackImportController(
    private val application:Application, private val scope:CoroutineScope,
    private val backend:StackImportBackend, private val created:suspend(String)->Unit,
){
    private val mutable=MutableStateFlow(StackImportState())
    val state:StateFlow<StackImportState> = mutable.asStateFlow()
    private var request:Job?=null
    private var read:DataOperation?=null
    private var creating=false
    fun edit(id:String=mutable.value.id,document:String=mutable.value.document){
        if(creating)return
        cancelPending()
        if(document.toByteArray(Charsets.UTF_8).size>MAX_STACK_DOCUMENT_BYTES){
            mutable.update{it.copy(revision=it.revision+1,preview=null,requirements=null,busy=false,error=R.string.group_document_large)};return
        }
        mutable.update{it.copy(id=id.take(64),document=document,revision=it.revision+1,preview=null,requirements=null,busy=false,error=null,detail=null,createdId=null)}
    }
    fun load(uri:Uri){
        if(creating)return
        cancelPending()
        val operation=DataOperation(DataTransferControl());read=operation
        val revision=mutable.value.revision+1
        mutable.update{it.copy(revision=revision,preview=null,requirements=null,busy=true,error=null,detail=null,createdId=null)}
        request=scope.launch{
            try{
                val document=withContext(Dispatchers.IO){
                    val input=operation.external(application.contentResolver.openInputStream(uri)?:error("document"))
                    input.use{readStackDocument(it)}
                }
                if(mutable.value.revision==revision)mutable.update{it.copy(document=document,busy=false)}
            }catch(failure:Exception){
                if(failure is CancellationException)throw failure
                if(mutable.value.revision==revision)mutable.update{it.copy(busy=false,error=when(failure){
                    is StackDocumentTooLarge->R.string.group_document_large
                    is java.nio.charset.CharacterCodingException->R.string.group_document_utf8
                    else->R.string.group_document_unavailable
                })}
            }finally{withContext(NonCancellable+Dispatchers.IO){operation.finish()};if(read===operation)read=null}
        }
    }
    fun preview(){
        if(creating||mutable.value.busy)return
        val draft=mutable.value
        mutable.update{it.copy(busy=true,preview=null,requirements=null,error=null,detail=null)}
        request=scope.launch{
            try{
                val requirements=backend.requirements(draft.document)
                if(mutable.value.revision!=draft.revision)return@launch
                mutable.update{it.copy(requirements=requirements)}
                val plan=backend.preview(draft.id,draft.document)
                if(mutable.value.revision==draft.revision)mutable.update{it.copy(busy=false,preview=plan)}
            }catch(failure:Exception){
                if(failure is CancellationException)throw failure
                if(mutable.value.revision==draft.revision)mutable.update{it.copy(busy=false,error=R.string.group_preview_failed,detail=privateError(failure))}
            }
        }
    }
    fun create(){
        val draft=mutable.value
        if(creating||draft.busy||draft.preview==null)return
        creating=true;mutable.update{it.copy(busy=true,error=null,detail=null)}
        request=scope.launch{
            try{
                // Pin the reviewed normalized plan. Revalidation must reject
                // a changed installed artifact instead of choosing a new one.
                backend.create(draft.id,draft.document,draft.preview)
                created(draft.id)
                creating=false
                mutable.value=StackImportState(createdId=draft.id,revision=draft.revision+1)
            }catch(failure:Exception){
                if(failure is CancellationException)throw failure
                mutable.update{it.copy(busy=false,preview=null,error=R.string.group_create_failed,detail=privateError(failure))}
            }finally{creating=false}
        }
    }
    fun clear(){if(!creating){cancelPending();mutable.value=StackImportState(revision=mutable.value.revision+1)}}
    fun leave(){if(!creating){cancelPending();mutable.update{it.copy(revision=it.revision+1,busy=false,preview=null,requirements=null)}}}
    private fun cancelPending(){read?.cancel();request?.cancel();request=null}
    fun close(){read?.cancel();request?.cancel()}
    private fun privateError(failure:Throwable):String{
        val actual=if(failure is java.util.concurrent.CompletionException)failure.cause?:failure else failure
        return (actual.message?:"Operation failed").replace(Regex("(?:https?|content|file)://\\S+"),"[resource]").take(2048)
    }
}
internal interface StackImportBackend{
    suspend fun requirements(document:String):String
    suspend fun preview(id:String,document:String):String
    suspend fun create(id:String,document:String,preview:String)
}
internal class StackDocumentTooLarge:IllegalArgumentException()
internal const val MAX_STACK_DOCUMENT_BYTES=128*1024
/** Reads one bounded document, with strict UTF-8 rather than replacement chars. */
internal fun readStackDocument(input:java.io.InputStream):String{
    val bytes=ByteArrayOutputStream();val buffer=ByteArray(8192)
    while(true){
        val count=input.read(buffer,0,minOf(buffer.size,MAX_STACK_DOCUMENT_BYTES+1-bytes.size()))
        if(count<0)break
        if(count==0){val byte=input.read();if(byte<0)break;bytes.write(byte)}else bytes.write(buffer,0,count)
        if(bytes.size()>MAX_STACK_DOCUMENT_BYTES)throw StackDocumentTooLarge()
    }
    return Charsets.UTF_8.newDecoder().onMalformedInput(CodingErrorAction.REPORT)
        .onUnmappableCharacter(CodingErrorAction.REPORT).decode(ByteBuffer.wrap(bytes.toByteArray())).toString().removePrefix("\uFEFF")
}
