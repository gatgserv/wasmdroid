package dev.wasmdroid.app.ui

import org.json.JSONObject
import dev.wasmdroid.app.engine.PackageProgress

data class PackageTask(
    val key:String,val generation:String,val kind:String,val label:String,val status:String,
    val reference:String?=null,val phase:String?=null,val bytes:Long=0,val total:Long=0,
    val imageCommitted:Boolean=false,val errorKind:String?=null,val technicalError:String?=null,
    val finished:Boolean=false,
){
    val identity:String get()="$key\u0000$generation"
    val terminal:Boolean get()=finished
    val cancellable:Boolean get()=!finished&&status in setOf("active","awaiting_review")
    val retryable:Boolean get()=kind!="local_import"&&finished&&status in setOf("cancelled","failed")
    internal val downloadedRegistryReference: String? get() =
        if (kind == "image" && terminal && status == "installed" && key.startsWith("pull:wasmer.io/")) {
            key.removePrefix("pull:")
        } else null
    companion object{
        internal fun parse(value:JSONObject,finished:Boolean):PackageTask?=runCatching{
            val key=value.getString("key");val generation=value.getString("generation");val status=value.getString("status")
            require(key.length in 1..2048&&generation.length in 1..128)
            require(status in setOf("active","awaiting_review","cancelled","publishing","committed","preparing_app","installed","failed"))
            require(!finished||status in setOf("installed","cancelled","failed"))
            val progress=value.optJSONObject("progress")?.toString()?.let(PackageProgress::parse)
            PackageTask(key,generation,value.getString("kind"),value.optString("label").take(160),status,
                reference=progress?.reference,phase=progress?.phase,bytes=progress?.bytes?:0,
                total=progress?.total?:0,imageCommitted=value.optBoolean("image_committed"),
                errorKind=value.optString("error_kind").ifBlank{null},technicalError=value.optString("technical_error").ifBlank{null}?.take(2048),finished=finished)
        }.getOrNull()
    }
}

internal fun AppState.packageWorkActive()=packages.any{!it.terminal&&it.status!="awaiting_review"}||busy.any{it=="install"||it.startsWith("pull:")||it.startsWith("package:retry:")}
internal fun packageBusyKey(key:String)=key=="install"||key.startsWith("pull:")||key.startsWith("package:")

/** A complete current-owner snapshot replaces prior frames; progress is never
 * joined to a different generation by key alone. Data transfer state is separate. */
internal fun packageTasks(raw:String):List<PackageTask>{
    val frame=JSONObject(raw);val active=frame.getJSONObject("active")
    val rows=linkedMapOf<String,PackageTask>()
    active.keys().asSequence().sorted().forEach{key->PackageTask.parse(active.getJSONObject(key),false)?.let{rows[key]=it}}
    val recent=frame.getJSONArray("recent")
    for(index in recent.length()-1 downTo 0){
        PackageTask.parse(recent.getJSONObject(index),true)?.let{if(it.key !in rows)rows[it.key]=it}
    }
    return rows.values.toList()
}
