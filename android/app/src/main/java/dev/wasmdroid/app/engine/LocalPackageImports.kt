package dev.wasmdroid.app.engine

import android.content.ContentResolver
import android.content.Context
import android.net.Uri
import android.os.CancellationSignal
import android.os.ParcelFileDescriptor
import android.provider.OpenableColumns
import dev.wasmdroid.engine.EngineStorage
import dev.wasmdroid.engine.InstallControl
import dev.wasmdroid.engine.InstallStatus
import dev.wasmdroid.engine.LocalPackageTransfer
import org.json.JSONObject
import java.io.InputStream
import java.util.UUID
import java.util.concurrent.CompletableFuture
import java.util.concurrent.Executor
import java.util.concurrent.Executors

internal const val LOCAL_PACKAGE_MAX_BYTES = 512L * 1024 * 1024
internal data class LocalPackageMetadata(val name:String,val size:Long?)
/** A host-owned input capability, never passed to the native runtime/guest. */
internal interface LocalPackageInput : AutoCloseable {
    fun open():LocalPackageMetadata
    fun read(buffer:ByteArray):Int
    fun cancel()
}
internal class DocumentPackageInput(private val resolver:ContentResolver,private val uri:Uri):LocalPackageInput {
    private val signal=CancellationSignal()
    private val lock=Any()
    private var stream:InputStream?=null
    private var cancelled=false
    override fun open():LocalPackageMetadata {
        require(uri.scheme=="content"){"A document provider selection is required"}
        var name="";var size:Long?=null
        // Some providers omit metadata. Bytes, not MIME/name, decide the format.
        try {
            resolver.query(uri,arrayOf(OpenableColumns.DISPLAY_NAME,OpenableColumns.SIZE),null,null,null,signal)?.use{cursor->
                if(cursor.moveToFirst()){
                    val nameIndex=cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                    if(nameIndex>=0&&!cursor.isNull(nameIndex))name=cursor.getString(nameIndex).filterNot{it.isISOControl()}.take(160)
                    val sizeIndex=cursor.getColumnIndex(OpenableColumns.SIZE)
                    if(sizeIndex>=0&&!cursor.isNull(sizeIndex))size=cursor.getLong(sizeIndex).takeIf{it>=0}
                }
            }
        }catch(failure:Exception){if(signal.isCanceled)throw failure}
        val descriptor=resolver.openFileDescriptor(uri,"r",signal)?:error("Document cannot be opened")
        val input=ParcelFileDescriptor.AutoCloseInputStream(descriptor)
        synchronized(lock){
            if(cancelled){input.close();error("Document read cancelled")}
            stream=input
        }
        if(size==null)size=descriptor.statSize.takeIf{it>=0}
        return LocalPackageMetadata(name,size)
    }
    override fun read(buffer:ByteArray):Int {
        val current=synchronized(lock){stream}?:error("Document input is closed")
        return current.read(buffer)
    }
    override fun close(){val current=synchronized(lock){stream.also{stream=null}};current?.close()}
    override fun cancel(){
        synchronized(lock){cancelled=true}
        try{close()}finally{signal.cancel()}
    }
}

/** Actor-owned state; only bounded IO work runs on the executor. No thread waits
 * while a human reviews a package. Callbacks never publish URIs or provider errors. */
internal class LocalPackageImports(
    private val context:Context,
    private val storage:CompletableFuture<EngineStorage>,
    private val io:Executor,
    private val post:((()->Unit)->Unit),
    private val admit:(PackageOperation)->Unit,
    private val finish:(PackageOperation,String,String?)->Unit,
) {
    private class Import(val operation:PackageOperation,val source:LocalPackageInput){
        @Volatile var transfer:LocalPackageTransfer?=null
        @Volatile var name=""
        var preview:String?=null
        var busy=true
        var errorKind:String?=null
    }
    private val active=linkedMapOf<String,Import>()
    private val recent=linkedMapOf<String,String>()
    // Provider cancellation/close may call Binder. It must not block the actor.
    private val canceller=Executors.newSingleThreadExecutor()
    fun begin(source:LocalPackageInput):String {
        val key="package:local:${UUID.randomUUID()}"
        val operation=PackageOperation(key,"local_import","",InstallControl()){
            CompletableFuture<String>().apply {
                completeExceptionally(IllegalStateException("Select the local document again"))
            }
        }
        try{admit(operation)}catch(failure:Exception){operation.control.close();throw failure}
        val record=Import(operation,source)
        active[key]=record
        operation.localProgress("reading_local",0,0)
        work(record,{db->
            val metadata=source.open()
            record.name=metadata.name.filterNot{it.isISOControl()}.take(160)
            require(metadata.size==null||metadata.size in 1..LOCAL_PACKAGE_MAX_BYTES){"Local package file size exceeds limits"}
            val available=context.filesDir.availableTransferBytes().toULong()
            record.transfer=db.beginPackageImport(metadata.size?.toULong(),available,null,operation.control)
            val buffer=ByteArray(64*1024);var total=0L;var emptyReads=0
            try{
                while(true){
                    check(operation.control.status()!=InstallStatus.CANCELLED){"Local import cancelled"}
                    val count=source.read(buffer)
                    if(count<0)break
                    if(count==0){check(++emptyReads<=32){"Document provider returned no bytes"};continue}
                    require(count<=buffer.size){"Invalid provider read"};emptyReads=0
                    total=Math.addExact(total,count.toLong())
                    require(total<=LOCAL_PACKAGE_MAX_BYTES){"Local package file size exceeds limits"}
                    record.transfer!!.write(buffer.copyOf(count))
                    operation.localProgress("reading_local",total,metadata.size?:0)
                }
            }finally{source.close()}
            operation.localProgress("inspecting_local",total,metadata.size?:0)
            record.transfer!!.preview().also{require(it.toByteArray(Charsets.UTF_8).size<=128*1024){"Local preview exceeds limit"}}
        },{preview->record.preview=preview;record.busy=false;operation.stage.set("awaiting_review")})
        return JSONObject().put("key",key).put("generation",operation.generation).toString()
    }
    fun frame(key:String,generation:String):String {
        active[key]?.takeIf{it.operation.generation==generation}?.let{return frame(it,it.operation.status())}
        return recent[key]?.takeIf{JSONObject(it).getString("generation")==generation}?:error("Local import is no longer retained")
    }
    fun review(key:String,generation:String){
        val record=current(key,generation)
        check(!record.busy&&record.operation.control.status()==InstallStatus.ACTIVE){"Local import is busy or closed"}
        record.busy=true;record.preview=null;record.errorKind=null;record.operation.stage.set(null)
        record.operation.localProgress("inspecting_local",0,0)
        work(record,{record.transfer!!.preview()}, {preview->
            require(preview.toByteArray(Charsets.UTF_8).size<=128*1024){"Local preview exceeds limit"}
            record.preview=preview;record.busy=false;record.operation.stage.set("awaiting_review")
        })
    }
    fun commit(key:String,generation:String,token:String){
        val record=current(key,generation)
        val preview=record.preview?.let(::JSONObject)?:error("Review the package first")
        check(!record.busy&&preview.getString("token")==token&&preview.getJSONArray("missing").length()==0){"Local package review changed or dependencies are missing"}
        check(record.operation.control.status()==InstallStatus.ACTIVE){"Local import is closed"}
        record.busy=true;record.errorKind=null;record.operation.stage.set(null)
        record.operation.localProgress("installing_local",0,0)
        CompletableFuture.supplyAsync({record.transfer!!.commit(token)},io).whenComplete{_,failure->post{
            if(failure!=null&&record.operation.control.status()==InstallStatus.ACTIVE){
                // Pre-publication failures keep the sealed input. Never install
                // again without displaying a fresh dependency review/token.
                record.preview=null;record.errorKind=localFailure(failure);record.busy=false
                record.operation.stage.set("awaiting_review")
            }else cleanup(record,if(failure==null)"installed" else outcome(record),failure?.let(::localFailure))
        }}
    }
    /** Called only after the shared native control accepted cancellation. */
    fun cancelled(key:String,generation:String){
        val record=active[key]?.takeIf{it.operation.generation==generation}?:return
        canceller.execute{runCatching{record.source.cancel()}}
        if(!record.busy){record.busy=true;cleanup(record,"cancelled",null)}
    }
    fun forget(key:String,generation:String){
        if(recent[key]?.let{JSONObject(it).getString("generation")==generation}==true)recent.remove(key)
    }
    fun shutdown(){check(active.isEmpty());recent.clear();canceller.shutdown()}
    private fun current(key:String,generation:String)=active[key]?.takeIf{it.operation.generation==generation}?:error("Local import generation changed")
    private fun frame(record:Import,status:String)=JSONObject().put("key",record.operation.key)
        .put("generation",record.operation.generation).put("name",record.name).put("status",status)
        .put("busy",record.busy).put("preview",record.preview?.let(::JSONObject)?:JSONObject.NULL)
        .put("error_kind",record.errorKind?:JSONObject.NULL).toString()
    private fun outcome(record:Import)=if(record.operation.control.status()==InstallStatus.CANCELLED)"cancelled" else "failed"
    private fun work(record:Import,action:(EngineStorage)->String,success:(String)->Unit){
        CompletableFuture.supplyAsync({action(storage.join())},io).whenComplete{value,failure->post{
            if(failure!=null)cleanup(record,outcome(record),localFailure(failure))
            else if(record.operation.control.status()==InstallStatus.CANCELLED)cleanup(record,"cancelled",null)
            else try{success(value)}catch(problem:Exception){cleanup(record,"failed",localFailure(problem))}
        }}
    }
    private fun cleanup(record:Import,status:String,error:String?){
        record.busy=true
        CompletableFuture.runAsync({
            // Dropping a failed native capability marks failure and releases
            // staging. abort() is reserved for actual cancellation, otherwise
            // a format/provider error would turn into a fake cancel outcome.
            try{record.transfer?.let{try{if(status=="cancelled")it.abort()}finally{it.close()}}}
            finally{record.transfer=null;record.source.close()}
        },io).whenComplete{_,failure->post{
            val actual=when{
                status=="installed"->status
                record.operation.control.status()==InstallStatus.CANCELLED->"cancelled"
                failure!=null->"failed"
                else->status
            }
            record.busy=false;record.errorKind=error?:failure?.let(::localFailure)
            recent[record.operation.key]=frame(record,actual)
            while(recent.size>16)recent.remove(recent.keys.first())
            active.remove(record.operation.key)
            finish(record.operation,actual,record.errorKind)
        }}
    }
}
private fun localFailure(failure:Throwable):String {
    val kind=packageFailure(failure).first
    return if(kind=="other")"local_input" else kind
}
