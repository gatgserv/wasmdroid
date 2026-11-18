package dev.wasmdroid.app.ui

import org.json.JSONObject

internal object GroupNetworkPermissionCodec{
    fun parse(raw:String):GroupPermissionSource{
        require(raw.toByteArray(Charsets.UTF_8).size<=32*1024)
        val root=JSONObject(raw)
        fun bool(key:String)=root.get(key).let{require(it is Boolean);it}
        fun revision(key:String)=root.get(key).let{require(it is Number&&it.toLong()>=0&&it.toString()==it.toLong().toString());it.toLong().toULong()}
        fun choice(key:String):Boolean?=when(root.getString(key)){"off"->false;"allow"->true;"mixed"->null;else->error("Invalid permission choice")}
        val target=root.getJSONObject("target");require(target.getString("kind")=="group")
        val members=root.getJSONArray("members");require(members.length() in 1..8)
        val values=(0 until members.length()).map{index->
            val member=members.getJSONObject(index);val version=member.getLong("version");require(version>=0)
            GroupPermissionMember(member.getString("id"),member.getString("identity"),member.getString("service"),version.toULong())
        }
        require(values.map{it.id}.distinct().size==values.size&&values.map{it.identity}.distinct().size==values.size)
        require(values.all{it.id.length in 1..128&&it.identity.length in 1..256&&it.service.length in 1..128})
        val name=root.getString("name");require(name.length in 1..256)
        return GroupPermissionSource(target.toString(),name,revision("version"),revision("grant_revision"),values,
            choice("internet"),choice("lan"),bool("can_edit"),bool("network_enabled"),bool("supported"))
    }
}
