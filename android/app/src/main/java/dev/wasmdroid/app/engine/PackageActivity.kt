package dev.wasmdroid.app.engine

import org.json.JSONObject
import java.util.UUID
import java.util.concurrent.CompletableFuture
import java.util.concurrent.atomic.AtomicReference
import dev.wasmdroid.engine.InstallControl

/** Host-only state. Snapshots omit recipe/env, download URLs and retry closures. */
internal class PackageOperation(
    val key:String,val kind:String,val label:String,val control:InstallControl,
    val retry:()->CompletableFuture<String>,
){
    val generation=UUID.randomUUID().toString()
    private val progress=AtomicReference<PackageProgress?>(null)
    val stage=AtomicReference<String?>(null)
    fun progress(raw:String){PackageProgress.parse(raw)?.let{progress.set(it)}}
    fun localProgress(phase:String,bytes:Long,total:Long){progress.set(PackageProgress("",phase,bytes,total))}
    fun status():String{
        val native=control.status().toString().lowercase(java.util.Locale.ROOT)
        return if(native in setOf("cancelled","failed","publishing"))native else stage.get()?:native
    }
    fun snapshot(status:String=status()):JSONObject=JSONObject().put("key",key).put("kind",kind)
        .put("label",label.take(160)).put("generation",generation).put("status",status)
        .put("progress",progress.get()?.json()?:JSONObject.NULL)
}
internal data class PackageProgress(val reference:String,val phase:String,val bytes:Long,val total:Long){
    fun json()=JSONObject().put("reference",reference).put("phase",phase).put("bytes_downloaded",bytes).put("total_bytes",total)
    companion object{
        fun parse(raw:String):PackageProgress?=runCatching{
            if(raw.length>16*1024)return null
            val value=JSONObject(raw);val phase=value.getString("phase")
            val local=phase in setOf("reading_local","inspecting_local","installing_local")
            if(!local&&phase !in setOf("resolving","downloading","verifying","installed"))return null
            val reference=value.getString("reference")
            if(local){if(reference.isNotEmpty())return null}
            else if(reference.length>1024||!reference.startsWith("wasmer.io/")||reference.any{it.isISOControl()}||reference.contains('?')||reference.contains('#'))return null
            val bytes=value.getLong("bytes_downloaded");val total=value.getLong("total_bytes")
            if(bytes<0||total<0||(total>0&&bytes>total)||(!local&&bytes>total)||(local&&(bytes>LOCAL_PACKAGE_MAX_BYTES||total>LOCAL_PACKAGE_MAX_BYTES)))return null
            PackageProgress(reference,phase,bytes,total)
        }.getOrNull()
    }
}
internal data class PackageReceipt(val json:String,val retry:()->CompletableFuture<String>)
/** Provenance for a registered operation, distinct from admission failures. */
internal class PackageActionFailure(val key:String,val generation:String,cause:Throwable):Exception(cause.message,cause)
internal fun packageFailure(failure:Throwable):Pair<String,String>{
    var actual=failure
    // Error reporting runs on the actor; even a cyclic external observer cause
    // chain must not stall control of running services.
    repeat(8){actual.cause?.let{actual=it}}
    val text=(actual.message?:"Operation failed").replace(Regex("(?:https?|content|file)://\\S+"),"[resource]").take(2048)
    val kind=when{
        text.contains("operation cancelled",true)->"cancelled"
        text.contains("space",true)||text.contains("os error 28")->"space"
        text.contains("cache writer",true)||text.contains("already active",true)||text.contains("installation is active",true)->"busy"
        text.contains("digest",true)->"integrity"
        text=="unsupported capability: registry must provide WebC v3"->"registry_webc_version"
        text.contains("unsupported",true)->"unsupported"
        text.contains("network",true)||text.contains("connection",true)||text.contains("DNS",true)->"network"
        else->"other"
    }
    return kind to text
}
