package dev.wasmdroid.app.shortcuts

import org.json.JSONObject

/** Public metadata only. Names, labels and caller URLs never grant authority. */
internal data class ShortcutTarget(val kind:String,val id:String,val identityToken:String){
    init{
        require(kind=="application"||kind=="group")
        require(id.isNotEmpty()&&id.length<=256)
        require(identityToken.isNotEmpty()&&identityToken.length<=512)
    }
    fun json():String=JSONObject().put("kind",kind).put("id",id).put("identity_token",identityToken).toString()
    fun sameIdentity(other:ShortcutTarget):Boolean=kind==other.kind&&identityToken==other.identityToken&&
        (kind=="application"||id==other.id)
    companion object{
        fun parse(json:String):ShortcutTarget{
            require(json.length<=2048)
            val value=JSONObject(json)
            return ShortcutTarget(value.getString("kind"),value.getString("id"),value.getString("identity_token"))
        }
    }
}

internal data class ShortcutDescriptor(
    val target:ShortcutTarget,val name:String,val state:String,val stateToken:String,
    val canStart:Boolean,val canStop:Boolean,val canOpen:Boolean,
){
    init{
        require(name.length<=512)
        require(state in setOf("stopped","starting","running","partial","stopping","unavailable"))
        require(stateToken.isNotEmpty()&&stateToken.length<=4096)
    }
    fun allows(action:ShortcutAction):Boolean=when(action){
        ShortcutAction.Start->canStart
        ShortcutAction.Stop->canStop
        ShortcutAction.Open->canOpen
        ShortcutAction.Review->state=="partial"
    }
    companion object{
        fun parse(json:String):ShortcutDescriptor{
            require(json.length<=16384)
            val value=JSONObject(json)
            return ShortcutDescriptor(ShortcutTarget.parse(value.getJSONObject("target").toString()),
                value.getString("name"),value.getString("state"),value.getString("state_token"),
                value.getBoolean("can_start"),value.getBoolean("can_stop"),value.getBoolean("can_open"))
        }
    }
}

internal enum class ShortcutAction(val wire:String){
    Start("start"),Stop("stop"),Open("open"),Review("review");
    companion object{fun parse(value:String)=entries.firstOrNull{it.wire==value}?:error("Unknown shortcut action")}
}

internal data class ShortcutSelection(val scope:String,val version:Long,val token:String,val target:ShortcutTarget?){
    init{require(validScope(scope));require(version>0);require(token.length in 1..128)}
    fun json():String=JSONObject().put("scope",scope).put("version",version.toString()).put("token",token)
        .put("target",target?.let{JSONObject(it.json())}?:JSONObject.NULL).toString()
    companion object{
        fun validScope(value:String):Boolean=value=="tile"||value.removePrefix("widget:").let{
            value.startsWith("widget:")&&it.toIntOrNull()?.let{number->number>0&&number.toString()==it}==true
        }
        fun widget(id:Int):String{require(id>0);return "widget:$id"}
        fun parse(json:String):ShortcutSelection{
            require(json.length<=4096)
            val value=JSONObject(json)
            return ShortcutSelection(value.getString("scope"),value.getString("version").toLong(),value.getString("token"),
                value.optJSONObject("target")?.let{ShortcutTarget.parse(it.toString())})
        }
    }
}

/** Immutable user click snapshot. It cannot be rebound to the latest selection. */
internal data class ShortcutClick(val selection:ShortcutSelection,val action:ShortcutAction,val stateToken:String){
    init{require(selection.target!=null);require(stateToken.isNotEmpty()&&stateToken.length<=4096)}
    fun json():String=JSONObject().put("selection",JSONObject(selection.json())).put("action",action.wire)
        .put("state_token",stateToken).toString()
    companion object{
        fun parse(json:String):ShortcutClick{
            require(json.length<=12288)
            val value=JSONObject(json)
            return ShortcutClick(ShortcutSelection.parse(value.getJSONObject("selection").toString()),
                ShortcutAction.parse(value.getString("action")),value.getString("state_token"))
        }
    }
}

/** Rechecked at actual owner admission, including after asynchronous preflight. */
internal class ShortcutAdmission(
    private val click:ShortcutClick,private val selectionCurrent:()->Boolean,
    private val visible:()->Boolean,private val unlocked:()->Boolean,
){
    fun current():Boolean=runCatching{visible()&&unlocked()&&selectionCurrent()}.getOrDefault(false)
    fun resolves(descriptor:ShortcutDescriptor):Boolean=click.selection.target?.sameIdentity(descriptor.target)==true&&
        descriptor.stateToken==click.stateToken&&descriptor.allows(click.action)
}
