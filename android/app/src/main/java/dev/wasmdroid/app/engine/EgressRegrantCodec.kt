package dev.wasmdroid.app.engine

import org.json.JSONObject

internal object EgressRegrantCodec{
    private fun frame(raw:String):JSONObject{require(raw.toByteArray(Charsets.UTF_8).size<=16*1024);return JSONObject(raw)}
    private fun JSONObject.flag(key:String)=get(key).let{require(it is Boolean);it}
    private fun JSONObject.epoch(key:String)=get(key).let{require(it is Number&&it.toLong()>0&&it.toString()==it.toLong().toString());it.toLong()}
    fun attachment(raw:String):EgressAttachment{
        val root=frame(raw)
        val current=if(root.isNull("current"))null else root.getJSONObject("current").let{value->
            val report=value.getJSONObject("report")
            EgressCurrent(value.epoch("grant_epoch"),value.epoch("network_epoch"),report.flag("closed"),report.flag("uncertain"))
        }
        return EgressAttachment(current,root.flag("ready"),root.flag("closed"),root.flag("uncertain"),root.flag("stopped"))
    }
    fun replacement(raw:String):EgressReplacementStatus{
        val root=frame(raw)
        return EgressReplacementStatus(root.epoch("grant_epoch"),root.epoch("network_epoch"),root.flag("admitted"),root.flag("terminal"),root.flag("closed"),root.flag("uncertain"))
    }
}
