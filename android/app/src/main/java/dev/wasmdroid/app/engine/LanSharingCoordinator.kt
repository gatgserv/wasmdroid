package dev.wasmdroid.app.engine

import android.content.Context
import android.os.Bundle
import android.os.Handler
import org.json.JSONArray
import org.json.JSONObject
import java.util.IdentityHashMap
import java.util.UUID
import java.util.concurrent.CompletableFuture

/** Internal seam for actor/acknowledgement regressions. The real worker still
 * independently re-attests the selected Android network before binding. */
internal interface LanMonitor : AutoCloseable {
    fun start()
    fun candidates():List<LanWifiCandidate>
    fun capture(candidate:LanWifiCandidate):LanNetworkCapture
}

/** Actor-confined transient state. Every relay stays in its actual worker;
 * this coordinator retains network captures until real cleanup/death evidence. */
internal class LanSharingCoordinator(context:Context,private val actor:Handler,
    private val admitted:()->Boolean,
    private val current:(Endpoint)->Boolean,
    private val command:(Endpoint,Int,Bundle,()->Boolean)->CompletableFuture<String>,
    private val revoke:(Endpoint,Bundle)->Unit,
    monitorFactory:((()->Unit)->LanMonitor)?=null,
) : AutoCloseable {
    data class Endpoint(val id:String,val generation:String,val port:Int,val path:String)
    private class Grant(val endpoint:Endpoint,val epoch:Long,val capture:LanNetworkCapture,val scope:String,val intent:LanStartIntent) {
        var phase="starting"
        var port:Int?=null
        var ipv6Address: String? = null
        var failed=false
        var cancelled=false
        var dispatched=false
        var cancelObserver:AutoCloseable?=null
        var cleanup:CompletableFuture<String>?=null
        var refresh:CompletableFuture<String>?=null
    }
    private val grants=linkedMapOf<Endpoint,Grant>()
    // A true value consumes the initial automatic attempt for this generation.
    // A subsequent network loss, explicit stop or failure requires manual sharing.
    private val defaultsAttempted = linkedMapOf<Endpoint, Boolean>()
    private val terminal=linkedMapOf<Endpoint,JSONObject>()
    private val selections=IdentityHashMap<LanWifiCandidate,String>()
    private val networks:LanMonitor=monitorFactory?.invoke(::networkChanged)?:object:LanMonitor {
        private val selected=LanNetworkSelection(context,actor,admitted,::networkChanged)
        override fun start()=selected.start()
        override fun candidates()=selected.candidates()
        override fun capture(candidate:LanWifiCandidate)=selected.capture(candidate)
        override fun close()=selected.close()
    }
    private var nextGrant=0L
    private var closed=false
    private var monitorFailed=false

    private fun monitor(){
        if(!closed&&!monitorFailed)try{networks.start()}catch(_:RuntimeException){monitorFailed=true}
    }
    private fun networkChanged(){
        // Callback invalidation is synchronous; actual cleanup may finish later.
        if(closed)return
        grants.values.toList().filter{!it.capture.valid}.forEach{stop(it)}
        if (monitorFailed || defaultsAttempted.isEmpty() || !admitted()) return
        val candidate = networks.candidates().singleOrNull() ?: return
        defaultsAttempted.entries.filter { (endpoint, attempted) ->
            !attempted && endpoint !in grants && current(endpoint)
        }.forEach { (endpoint, _) ->
            defaultsAttempted[endpoint] = true
            runCatching { start(endpoint, candidate, admitted, LanStartIntent()) }
        }
    }
    fun readyDefault(endpoint: Endpoint) {
        if (closed || !current(endpoint)) return
        defaultsAttempted.putIfAbsent(endpoint, false)
        monitor()
        networkChanged()
    }
    fun admissionChanged() = networkChanged()
    private fun matchesIpv6(expected: String?, receipt: JSONObject): Boolean =
        when (val address = receipt.opt("ipv6_address")) {
            null, JSONObject.NULL -> expected == null
            is String -> LanSelectionLedger.sameIpv6Address(expected, address)
            else -> false
        }
    private fun bundle(grant:Grant)=Bundle().apply{putString(WorkerProtocol.LAN_SCOPE,grant.scope)}
    private fun observed(actual:CompletableFuture<String>):CompletableFuture<String> {
        val observer=CompletableFuture<String>()
        actual.whenComplete{value,failure->if(failure==null)observer.complete(value)else observer.completeExceptionally(failure)}
        return observer
    }
    private fun frame(endpoint:Endpoint?):JSONObject {
        monitor()
        val candidates=networks.candidates()
        selections.keys.toList().filter{prior->candidates.none{it===prior}}.forEach{selections.remove(it)}
        val choices=JSONArray()
        candidates.forEach{candidate->
            val token=selections.getOrPut(candidate){UUID.randomUUID().toString()}
            choices.put(JSONObject().put("selection_token",token).put("private_address",candidate.privateAddress)
                .put("ipv6_address", candidate.ipv6Address)
                .put("blocked_status_observed",candidate.blockedStatusObserved))
        }
        val grant=endpoint?.let{grants[it]}
        val frame=JSONObject().put("phase",when{
            endpoint==null||!current(endpoint)->"app_stopped"
            grant!=null->grant.phase
            monitorFailed->"unavailable"
            else->"off"
        }).put("candidates",choices).put("default_sharing", endpoint != null && endpoint in defaultsAttempted)
        if(grant==null&&endpoint!=null)terminal[endpoint]?.let{last->
            frame.put("last_grant_epoch",last.getLong("grant_epoch")).put("failed",last.getBoolean("failed"))
        }
        if(grant!=null){
            frame.put("grant_epoch",grant.epoch).put("failed",grant.failed)
            // Never render an address after a selection/generation invalidates.
            if(!grant.cancelled&&!grant.intent.cancelled&&grant.phase=="active"&&grant.capture.valid&&current(grant.endpoint)){
                grant.port?.let { port ->
                    frame.put("url", "http://${grant.capture.candidate.privateAddress}:$port${grant.endpoint.path}")
                    grant.ipv6Address?.let { address ->
                        frame.put("ipv6_url", "http://[$address]:$port${grant.endpoint.path}")
                    }
                }
            }
        }
        return frame
    }
    fun status(endpoint:Endpoint?):CompletableFuture<String> {
        // A permission can change independently of the Wi-Fi callback stream.
        networkChanged()
        val grant=endpoint?.let{grants[it]}
        if(grant==null||grant.phase=="starting"||grant.cleanup?.isDone==false)return CompletableFuture.completedFuture(frame(endpoint).toString())
        grant.refresh?.takeIf{!it.isDone}?.let{return observed(it)}
        val actual=CompletableFuture<String>();grant.refresh=actual
        try{
            command(grant.endpoint,WorkerProtocol.LAN_STATUS,bundle(grant),{true}).whenComplete{value,failure->actor.post{
                if(grants[grant.endpoint]!==grant){actual.complete(frame(endpoint).toString());return@post}
                try{
                    check(failure==null)
                    val state=JSONObject(value);check(state.getLong("grant_epoch")==grant.epoch)
                    when(state.getString("phase")){
                        "closed"->{recordClosed(grant,state.optBoolean("failed",false));grants.remove(grant.endpoint);grant.cancelObserver?.close();grant.capture.close()}
                        "active"->{
                            // A native query cannot undo explicit Off/revocation.
                            if(grant.cancelled)stop(grant)
                            else check(state.getString("private_address")==grant.capture.candidate.privateAddress&&state.getInt("host_port")==grant.port&&
                                matchesIpv6(grant.ipv6Address, state))
                        }
                        "preparing","revoking"->{grant.phase="stopping";grant.port=null}
                        else->{grant.failed=true;stop(grant)}
                    }
                }catch(_:Exception){grant.failed=true;stop(grant)}
                actual.complete(frame(endpoint).toString())
            }}
        }catch(_:Exception){grant.failed=true;stop(grant);actual.complete(frame(endpoint).toString())}
        return observed(actual)
    }
    fun enable(endpoint:Endpoint,selectionToken:String,admission:()->Boolean,startIntent:LanStartIntent=LanStartIntent()):CompletableFuture<String> {
        val candidate=selections.entries.singleOrNull{it.value==selectionToken}?.key?:error("Wi-Fi selection changed; review it again")
        return start(endpoint, candidate, admission, startIntent)
    }
    private fun start(
        endpoint: Endpoint, candidate: LanWifiCandidate, admission: () -> Boolean, startIntent: LanStartIntent,
    ): CompletableFuture<String> {
        check(!closed&&!startIntent.cancelled&&current(endpoint)&&admission()){"Application sharing selection changed"}
        check(!monitorFailed&&endpoint !in grants){"LAN sharing is unavailable or still changing"}
        check(nextGrant<Long.MAX_VALUE){"LAN grant epoch exhausted"}
        val capture=networks.capture(candidate)
        val epoch=++nextGrant
        val scope=JSONObject().put("instance_id",endpoint.id).put("generation",endpoint.generation)
            .put("instance_port",endpoint.port).put("grant_epoch",epoch).put("private_address",candidate.privateAddress)
            .put("ipv6_address",candidate.ipv6Address).put("ipv6_prefix_length",candidate.ipv6PrefixLength).toString()
        val grant=Grant(endpoint,epoch,capture,scope,startIntent)
        defaultsAttempted.computeIfPresent(endpoint) { _, _ -> true }
        terminal.remove(endpoint)
        grants[endpoint]=grant
        capture.onInvalidated{actor.post{if(grants[endpoint]===grant)stop(grant)}}
        val answer=CompletableFuture<String>()
        try{
            grant.cancelObserver=startIntent.onCancelled{actor.post{if(grants[endpoint]===grant)stop(grant)}}
            check(!startIntent.cancelled&&current(endpoint)&&capture.valid&&admission()){"LAN admission changed"}
            val request=bundle(grant).apply{putLong(WorkerProtocol.LAN_NETWORK,candidate.networkHandle)}
            command(endpoint,WorkerProtocol.LAN_ENABLE,request,{
                if(!grant.cancelled&&!startIntent.cancelled&&current(endpoint)&&capture.valid){grant.dispatched=true;true}else false
            }).whenComplete{value,failure->actor.post{
                if(grants[endpoint]!==grant){answer.completeExceptionally(IllegalStateException("LAN generation ended"));return@post}
                val receipt=if(failure==null)runCatching{JSONObject(value)}.getOrNull()else null
                val scopeMatches=receipt!=null&&runCatching{
                    receipt.getString("instance_id")==endpoint.id&&receipt.getString("generation")==endpoint.generation&&
                        receipt.getInt("instance_port")==endpoint.port&&receipt.getLong("grant_epoch")==epoch&&
                        receipt.getString("private_address")==candidate.privateAddress
                }.getOrDefault(false)
                if(scopeMatches&&receipt!!.optString("phase")=="closed"&&receipt.opt("native_admitted") is Boolean){
                    recordClosed(grant,receipt.optBoolean("failed",false));grants.remove(endpoint)
                    grant.cancelObserver?.close();capture.close()
                    answer.completeExceptionally(IllegalStateException("LAN sharing was not activated"))
                }else if(failure==null&&!grant.cancelled&&!startIntent.cancelled&&grant.phase=="starting"&&current(endpoint)&&capture.valid){
                    try{
                        val ack=JSONObject(value)
                        check(ack.getString("instance_id")==endpoint.id&&ack.getString("generation")==endpoint.generation&&
                            ack.getInt("instance_port")==endpoint.port&&ack.getLong("grant_epoch")==epoch&&
                            ack.getString("private_address")==candidate.privateAddress&&ack.getString("phase")=="active")
                        val port=ack.getInt("host_port");check(port in 1..65535)
                        val ipv6 = ack.opt("ipv6_address") as? String
                        check(matchesIpv6(candidate.ipv6Address, ack))
                        grant.port=port;grant.ipv6Address=ipv6;grant.phase="active";answer.complete(frame(endpoint).toString())
                    }catch(_:Exception){grant.failed=true;stop(grant);answer.completeExceptionally(IllegalStateException("LAN acknowledgement did not match the selected application"))}
                }else{
                    grant.failed=true;stop(grant)
                    answer.completeExceptionally(IllegalStateException("LAN sharing could not start; review the application and Wi-Fi status"))
                }
            }}
        }catch(failure:Exception){grant.failed=!startIntent.cancelled;stop(grant);answer.completeExceptionally(failure)}
        return observed(answer)
    }
    fun disable(endpoint:Endpoint,epoch:Long):CompletableFuture<String> {
        val grant=grants[endpoint]?:error("LAN sharing is no longer active")
        check(grant.epoch==epoch){"LAN grant changed; review it again"}
        return observed(stop(grant))
    }
    private fun stop(grant:Grant):CompletableFuture<String> {
        grant.cancelled=true
        grant.phase="stopping";grant.port=null;grant.ipv6Address=null
        if(!grant.dispatched){
            recordClosed(grant,grant.failed);grants.remove(grant.endpoint)
            grant.cancelObserver?.close();grant.capture.close()
            return CompletableFuture.completedFuture(JSONObject().put("phase","closed").put("grant_epoch",grant.epoch)
                .put("native_admitted",false).put("failed",grant.failed).toString())
        }
        revoke(grant.endpoint,bundle(grant))
        // A prior failed observer is not a terminal receipt; a visible retry
        // can ask the same exact native scope for actual completion again.
        grant.cleanup?.takeIf{!it.isDone}?.let{return it}
        val actual=CompletableFuture<String>();grant.cleanup=actual
        try{
            command(grant.endpoint,WorkerProtocol.LAN_DISABLE,bundle(grant),{true}).whenComplete{value,failure->actor.post{
                if(grants[grant.endpoint]!==grant){actual.complete("{\"phase\":\"closed\"}");return@post}
                if(failure==null&&runCatching{
                    val ack=JSONObject(value);ack.getString("phase")=="closed"&&ack.getLong("grant_epoch")==grant.epoch
                }.getOrDefault(false)){
                    recordClosed(grant,JSONObject(value).optBoolean("failed",false))
                    grants.remove(grant.endpoint);grant.cancelObserver?.close();grant.capture.close();actual.complete(value)
                }else{
                    grant.phase="uncertain";grant.failed=true
                    actual.completeExceptionally(IllegalStateException("LAN cleanup is not confirmed; stop the application or retry stopping sharing"))
                }
            }}
        }catch(_:Exception){grant.phase="uncertain";grant.failed=true;actual.completeExceptionally(IllegalStateException("LAN cleanup is not confirmed"))}
        return actual
    }
    private fun recordClosed(grant:Grant,failed:Boolean){
        terminal[grant.endpoint]=JSONObject().put("grant_epoch",grant.epoch).put("failed",failed)
        while(terminal.size>16)terminal.remove(terminal.keys.first())
    }
    fun stopping(id:String,generation:String){
        defaultsAttempted.keys.removeAll { it.id == id && it.generation == generation }
        grants.values.toList().filter{it.endpoint.id==id&&it.endpoint.generation==generation}.forEach{stop(it)}
    }
    /** Called only after actual worker death and native terminal publication. */
    fun ended(id:String,generation:String){
        defaultsAttempted.keys.removeAll { it.id == id && it.generation == generation }
        grants.values.toList().filter{it.endpoint.id==id&&it.endpoint.generation==generation}.forEach{grant->
            grants.remove(grant.endpoint);grant.cancelObserver?.close();grant.capture.close()
        }
    }
    override fun close(){
        closed=true
        defaultsAttempted.clear()
        grants.values.toList().forEach{stop(it)}
        networks.close();selections.clear();terminal.clear()
    }
}
