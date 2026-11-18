package dev.wasmdroid.app.ui

import org.json.JSONArray
import org.json.JSONObject

data class LocalPackageSelection(val key:String,val generation:String)
data class LocalPackageView(
    val identity:LocalPackageSelection,val name:String,val status:String,val busy:Boolean,
    val preview:LocalPackagePreview?,val errorKind:String?,
) {
    companion object {
        fun parse(raw:String):LocalPackageView {
            require(raw.toByteArray(Charsets.UTF_8).size<=132*1024)
            val frame=JSONObject(raw)
            val key=frame.getString("key");val generation=frame.getString("generation")
            require(key.startsWith("package:local:")&&key.length<=128&&generation.length in 1..128)
            val status=frame.getString("status")
            require(status in setOf("active","awaiting_review","publishing","committed","cancelled","installed","failed"))
            return LocalPackageView(LocalPackageSelection(key,generation),frame.optString("name").filterNot{it.isISOControl()}.take(160),
                status,frame.getBoolean("busy"),frame.optJSONObject("preview")?.let(LocalPackagePreview::parse),
                frame.optString("error_kind").takeIf{it in setOf("space","busy","integrity","unsupported","network","local_input","cancelled")})
        }
    }
}
data class LocalPackageDependency(val text:String,val query:String?)
data class LocalPackagePreview(val token:String,val reference:String,val digest:String,val kind:String,
    val commands:List<String>,val entrypoint:String?,val bytes:Long,val missing:List<LocalPackageDependency>,val pinCount:Int) {
    fun spec(command:String):String {
        require(command in commands)
        return JSONObject().put("artifact_ref",reference).put("command",command).put("args",JSONArray())
            .put("env",JSONObject()).put("mounts",JSONArray()).put("ports",JSONArray()).put("network","loopback").toString(2)
    }
    companion object {
        fun parse(value:JSONObject):LocalPackagePreview {
            require(value.getInt("schema_version")==1)
            val artifact=value.getJSONObject("artifact")
            val digest=artifact.getString("digest");val token=value.getString("token")
            val hash=Regex("sha256:[0-9a-f]{64}")
            require(hash.matches(digest)&&hash.matches(token))
            val reference=artifact.getString("reference")
            require(reference=="local.wasmdroid/sha256/${digest.removePrefix("sha256:")}")
            val kind=artifact.getString("kind");require(kind in setOf("wasm","webc")&&artifact.getString("driver")=="wasmer-local")
            val commands=artifact.getJSONArray("commands").let{array->(0 until array.length()).map{array.getString(it)}}
            require(commands.size in 1..256&&commands.distinct().size==commands.size&&commands.all{it.isNotEmpty()&&it.toByteArray(Charsets.UTF_8).size<=128&&!it.any(Char::isISOControl)})
            val bytes=artifact.getLong("size_bytes");require(bytes in 1..512L*1024*1024)
            val missingArray=value.getJSONArray("missing");require(missingArray.length()<=32)
            val missing=(0 until missingArray.length()).map{index->
                val item=missingArray.getJSONObject(index)
                when(item.getString("type")){
                    "named"->{
                        val ref=item.getString("reference");val version=item.getString("version_requirement")
                        require(ref.startsWith("wasmer.io/")&&ref.length<=270&&!ref.any(Char::isISOControl)&&version.length<=256&&!version.any(Char::isISOControl))
                        LocalPackageDependency("$ref $version",ref.removePrefix("wasmer.io/"))
                    }
                    "sha256"->{val dep=item.getString("digest");require(hash.matches(dep));LocalPackageDependency(dep,null)}
                    else->error("Unsupported local dependency requirement")
                }
            }
            val pins=value.getJSONArray("pins").length();require(pins<=31)
            val entrypoint=artifact.optJSONObject("labels")?.optString("dev.wasmdroid.entrypoint")?.takeIf{it in commands}
            return LocalPackagePreview(token,reference,digest,kind,commands,entrypoint,bytes,missing,pins)
        }
    }
}
