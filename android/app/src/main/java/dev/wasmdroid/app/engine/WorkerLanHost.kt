package dev.wasmdroid.app.engine

import android.content.Context
import android.os.Bundle
import android.os.Handler
import dev.wasmdroid.engine.LanSocketBinder
import dev.wasmdroid.engine.ServiceRuntime
import org.json.JSONObject
import java.util.concurrent.atomic.AtomicBoolean

/** Same-process bridge: parent messages select among actual local Network
 * capabilities, never authority to connect to a remembered host port. */
internal class WorkerLanHost(context:Context,handler:Handler,
    private val identity:()->Pair<String,String>?,private val admitted:()->Boolean,
    private val runtime:()->ServiceRuntime?) : AutoCloseable {
    internal sealed class EnableReservation
    private class Prepared(val grant:Grant):EnableReservation() {val consumed=AtomicBoolean(false)}
    private class Declined(val receipt:String):EnableReservation()
    private class Grant(val scope:String,val epoch:Long,val port:Int,val capture:LanNetworkCapture,
        val execution:ServiceRuntime) {val dispatch=LanEnableDispatch()}
    private val ownership=Any()
    private val grants=linkedMapOf<Int,Grant>()
    // Exact, bounded receipts cover a cancelled thread starting late without
    // mutating a newer grant on the same published guest port.
    private val terminal=linkedMapOf<String,String>()
    private val networks=LanNetworkSelection(context,handler,admitted,{})
    @Volatile private var monitorAvailable=false

    fun start(){monitorAvailable=runCatching{networks.start();true}.getOrDefault(false)}
    private fun scope(data:Bundle):Pair<String,JSONObject> {
        val raw=data.getString(WorkerProtocol.LAN_SCOPE)?:error("Missing private LAN scope")
        require(raw.toByteArray(Charsets.UTF_8).size<=2048){"Private LAN scope exceeds input limit"}
        val parsed=JSONObject(raw);val actual=identity()?:error("Worker identity is unavailable")
        val fields=setOf("instance_id","generation","instance_port","grant_epoch","private_address")
        val hasIpv6=parsed.has("ipv6_address")||parsed.has("ipv6_prefix_length")
        val allowed=if(hasIpv6)fields+setOf("ipv6_address","ipv6_prefix_length") else fields
        require(parsed.length()==allowed.size&&parsed.keys().asSequence().all{it in allowed}){"Invalid private LAN scope fields"}
        require(listOf("instance_id","generation","private_address").all{parsed.get(it) is String}){"Invalid private LAN scope strings"}
        require(parsed.get("instance_port") is Int&&parsed.get("grant_epoch").let{it is Int||it is Long}){"Invalid private LAN scope numbers"}
        check(parsed.getString("instance_id")==actual.first&&parsed.getString("generation")==actual.second){"LAN worker generation changed"}
        require(parsed.getInt("instance_port") in 1..65535&&parsed.getLong("grant_epoch")>0){"Invalid LAN grant"}
        require(LanSelectionLedger.privateIpv4(parsed.getString("private_address"))!=null){"Invalid private LAN address"}
        val canonical=JSONObject().put("instance_id",actual.first).put("generation",actual.second)
            .put("instance_port",parsed.getInt("instance_port")).put("grant_epoch",parsed.getLong("grant_epoch"))
            .put("private_address",parsed.getString("private_address"))
        if(hasIpv6){
            val address=parsed.get("ipv6_address")
            val prefix=parsed.get("ipv6_prefix_length")
            require(address is String&&prefix is Int){"Invalid private Wi-Fi IPv6 scope"}
            val ipv6=LanSelectionLedger.wifiIpv6(address,prefix)?:error("Invalid private Wi-Fi IPv6 address")
            canonical.put("ipv6_address",ipv6.address).put("ipv6_prefix_length",ipv6.prefixLength)
        }
        return canonical.toString() to canonical
    }
    private fun matchingGrant(parsed:JSONObject):Grant?=synchronized(ownership){
        val grant=grants[parsed.getInt("instance_port")]?:return@synchronized null
        check(grant.epoch==parsed.getLong("grant_epoch")&&
            grant.capture.candidate.privateAddress==parsed.getString("private_address")){"LAN grant scope changed"}
        val captured=JSONObject(grant.scope)
        check(captured.getString("instance_id")==parsed.getString("instance_id")&&
            captured.getString("generation")==parsed.getString("generation")&&
            captured.opt("ipv6_address")==parsed.opt("ipv6_address")&&
            captured.opt("ipv6_prefix_length")==parsed.opt("ipv6_prefix_length")){"LAN worker scope changed"}
        grant
    }
    private fun receipt(raw:String,nativeAdmitted:Boolean,failed:Boolean):String=JSONObject(raw)
        .put("phase","closed").put("native_admitted",nativeAdmitted).put("failed",failed).toString()
    private fun rememberLocked(raw:String,result:String){
        terminal[raw]=if(terminal[raw]?.let{JSONObject(it).optBoolean("failed",false)}==true)
            JSONObject(result).put("failed",true).toString() else result
        while(terminal.size>16)terminal.remove(terminal.keys.first())
    }
    private fun terminal(raw:String):String?=synchronized(ownership){terminal[raw]}
    private fun release(grant:Grant,result:String){
        val removed=synchronized(ownership){
            rememberLocked(grant.scope,result)
            if(grants[grant.port]===grant){grants.remove(grant.port);true}else false
        }
        if(removed)grant.capture.close()
    }
    private fun revoke(grant:Grant){
        grant.dispatch.cancel()
        if(!grant.dispatch.cancelledBeforeDispatch)runCatching{grant.execution.revokeLan(grant.scope)}
    }
    private fun cancelled(grant:Grant,failed:Boolean):String {
        check(grant.dispatch.cancelledBeforeDispatch){"LAN dispatch is still owned"}
        return receipt(grant.scope,false,failed).also{release(grant,it)}
    }
    private fun nativeClosed(grant:Grant,state:JSONObject,failed:Boolean):String {
        check(state.getString("phase")=="closed"&&state.getLong("grant_epoch")==grant.epoch){"LAN cleanup is not confirmed"}
        return receipt(grant.scope,true,failed||state.optBoolean("failed",false)).also{release(grant,it)}
    }
    /** Main-only capacity refusal, before any reservation/native admission.
     * An owned port (including an identical duplicate) cannot be declined as
     * closed. Invalid scopes never modify existing grants or receipts. */
    fun declineEnable(data:Bundle):String {
        val (raw,parsed)=scope(data)
        val result=receipt(raw,false,true)
        synchronized(ownership){
            check(parsed.getInt("instance_port") !in grants){"Previous LAN sharing cleanup is still pending"}
            rememberLocked(raw,result)
        }
        return result
    }
    /** Control Main only: no native call, bind, wait or background thread here. */
    fun reserveEnable(data:Bundle):EnableReservation {
        val (raw,parsed)=scope(data)
        val port=parsed.getInt("instance_port")
        // Identical duplicates cannot claim no native admission while an
        // existing grant may own a relay. Never revoke/replace that grant.
        synchronized(ownership){check(port !in grants){"Previous LAN sharing cleanup is still pending"}}
        terminal(raw)?.let{return Declined(it)}
        var capture:LanNetworkCapture?=null
        try{
            check(admitted()&&monitorAvailable){"Selected Wi-Fi status is unavailable"}
            val execution=runtime()?:error("Runtime is not ready")
            val selected=networks.captureMatching(data.getLong(WorkerProtocol.LAN_NETWORK),parsed.getString("private_address"),
                parsed.opt("ipv6_address") as? String,parsed.opt("ipv6_prefix_length") as? Int)
            capture=selected
            val grant=Grant(raw,parsed.getLong("grant_epoch"),port,selected,execution)
            synchronized(ownership){check(port !in grants){"LAN sharing admission changed"};grants[port]=grant}
            selected.onInvalidated{revoke(grant)}
            return Prepared(grant)
        }catch(_:Exception){
            capture?.close()
            synchronized(ownership){check(port !in grants){"LAN preparation remains owned"}}
            val result=receipt(raw,false,true)
            synchronized(ownership){rememberLocked(raw,result)}
            return Declined(result)
        }
    }
    /** Background thread consumes exactly the Main-reserved opaque handle. */
    fun enable(reservation:EnableReservation):String {
        if(reservation is Declined)return reservation.receipt
        val prepared=reservation as Prepared
        check(prepared.consumed.compareAndSet(false,true)){"LAN reservation was already consumed"}
        val grant=prepared.grant
        if(!grant.capture.valid)revoke(grant)
        if(!grant.dispatch.dispatch())return cancelled(grant,false)
        val capture=grant.capture
        try{
            val result=grant.execution.enableLan(grant.scope,object:LanSocketBinder{
                override fun bindSocket(descriptor:Int,grantEpoch:ULong,privateAddress:String):Boolean=runCatching{
                    check(grantEpoch==grant.epoch.toULong()&&privateAddress==capture.candidate.privateAddress)
                    check(grant.dispatch.allowed&&admitted()&&capture.valid)
                    networks.bind(capture,descriptor)
                    check(grant.dispatch.allowed&&admitted()&&capture.valid)
                    true
                }.getOrDefault(false)
            })
            check(grant.dispatch.allowed&&admitted()&&capture.valid){"Selected Wi-Fi changed during sharing admission"}
            return result
        }catch(failure:Exception){
            revoke(grant)
            val status=runCatching{JSONObject(grant.execution.lanStatus(grant.scope))}.getOrNull()
            if(status?.optString("phase")=="closed")return nativeClosed(grant,status,true)
            throw failure // Unknown/uncertain is never a fabricated Closed.
        }
    }
    fun disable(data:Bundle):String {
        val (raw,parsed)=scope(data)
        terminal(raw)?.let{return it}
        val grant=matchingGrant(parsed)?:error("LAN grant is unavailable")
        revoke(grant)
        if(grant.dispatch.cancelledBeforeDispatch)return cancelled(grant,false)
        val result=try{JSONObject(grant.execution.disableLan(raw))}catch(failure:Exception){
            val status=runCatching{JSONObject(grant.execution.lanStatus(raw))}.getOrNull()
            if(status?.optString("phase")!="closed")throw failure
            return nativeClosed(grant,status,true)
        }
        return nativeClosed(grant,result,false)
    }
    fun status(data:Bundle):String {
        val (raw,parsed)=scope(data)
        terminal(raw)?.let{return it}
        val grant=matchingGrant(parsed)?:error("LAN grant is unavailable")
        if(!grant.capture.valid)revoke(grant)
        if(grant.dispatch.cancelledBeforeDispatch)return cancelled(grant,false)
        if(grant.dispatch.reserved)return JSONObject(raw).put("phase","preparing").put("native_admitted",false).toString()
        val result=JSONObject(grant.execution.lanStatus(raw))
        if(result.getString("phase")=="closed")return nativeClosed(grant,result,false)
        return result.toString()
    }
    /** Control Main: immediate fence, never await relay cleanup. */
    fun revoke(data:Bundle){
        val (raw,parsed)=scope(data)
        if(terminal(raw)!=null)return
        matchingGrant(parsed)?.let(::revoke)?:error("LAN grant is unavailable")
    }
    fun revokeAll(){
        synchronized(ownership){grants.values.toList()}.forEach(::revoke)
        runCatching{runtime()?.revokeAllLan()}
        networks.admissionChanged()
    }
    override fun close(){revokeAll();networks.close()}
}
