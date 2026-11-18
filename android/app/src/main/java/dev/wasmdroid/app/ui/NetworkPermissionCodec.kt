package dev.wasmdroid.app.ui

import org.json.JSONObject
import org.json.JSONTokener

internal object NetworkPermissionCodec {
    fun parse(raw:String):NetworkPermissionSource {
        require(raw.length<=4096&&raw.toByteArray(Charsets.UTF_8).size<=4096&&'\u0000' !in raw)
        val tokens=JSONTokener(raw);val value=tokens.nextValue();require(value is JSONObject)
        require(tokens.nextClean()=='\u0000'&&!tokens.more())
        require(value.keys().asSequence().toSet()==setOf("id","identity","version","name","state","managed","network_enabled","internet","lan","supported"))
        fun text(key:String,maximum:Int)=value.get(key).let{require(it is String&&it.toByteArray(Charsets.UTF_8).size<=maximum);it}
        fun flag(key:String)=value.get(key).let{require(it is Boolean);it}
        val version=value.get("version");require(version is Int||version is Long)
        val number=(version as Number).toLong();require(number>=0)
        val id=text("id",128);val identity=text("identity",128);require(id.isNotBlank()&&identity.isNotBlank())
        val status=text("state",32);require(status in setOf("created","running","paused","restarting","exited","dead"))
        return NetworkPermissionSource(id,identity,number.toULong(),text("name",1024),status,
            flag("managed"),flag("network_enabled"),flag("internet"),flag("lan"),flag("supported"))
    }
}
