package dev.wasmdroid.app.engine

import android.content.Context
import android.net.*
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.ParcelFileDescriptor
import dev.wasmdroid.engine.WorkerEgressPlatform
import dev.wasmdroid.engine.WorkerEgressRevoker
import org.json.JSONArray
import org.json.JSONObject
import java.net.InetAddress
import java.net.NetworkInterface
import java.util.Collections
import java.util.concurrent.CompletableFuture
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

/** Worker-private actual default-Network ownership. It never requests Wi-Fi,
 * bypasses a VPN, binds the whole process, or performs an ambient DNS fallback.
 * Native calls use retained background provider work, never this control Main. */
internal class WorkerEgressHost(context:Context,private val main:Handler,private val admitted:()->Boolean):AutoCloseable {
    private val connectivity=context.getSystemService(ConnectivityManager::class.java)
    private val closed=AtomicBoolean(false)
    private val lock=Any()
    private var started=false
    private var epoch=0L
    private var observed:Observation?=null
    private val blocked=mutableMapOf<Network,Boolean>()
    private var pending:CompletableFuture<Capture>?=null
    private val captures=mutableSetOf<Capture>()
    private data class Observation(val network:Network,val own:List<String>,val onLink:List<String>,val nat64:List<String>,val contextKey:String){
        override fun toString()="EgressNetworkObservation(private)"
        fun signature()=own.joinToString("|")+";"+onLink.joinToString("|")+";"+nat64.joinToString("|")+";"+contextKey
    }
    private val defaults=object:ConnectivityManager.NetworkCallback(){
        override fun onAvailable(network:Network){refresh()}
        override fun onLost(network:Network){lose(network);blocked.remove(network);refresh()}
        override fun onLinkPropertiesChanged(network:Network,properties:LinkProperties){refresh()}
        override fun onCapabilitiesChanged(network:Network,capabilities:NetworkCapabilities){refresh()}
        override fun onBlockedStatusChanged(network:Network,isBlocked:Boolean){
            blocked[network]=isBlocked
            if(blocked.size>16)blocked.keys.retainAll(listOfNotNull(connectivity.activeNetwork))
            if(isBlocked)lose(network);refresh()
        }
    }
    // Non-default interface changes can alter a host-owned address; invalidate
    // the same private context even when the selected default is still a VPN.
    private val interfaces=object:ConnectivityManager.NetworkCallback(){
        override fun onAvailable(network:Network){refresh()}
        override fun onLost(network:Network){refresh()}
        override fun onLinkPropertiesChanged(network:Network,properties:LinkProperties){refresh()}
    }
    private fun numeric(address:InetAddress):String=checkNotNull(InetAddress.getByAddress(address.address).hostAddress)
    private fun observe():Observation? {
        val actual=observeActual()?:return null
        if(Build.VERSION.SDK_INT>=29&&blocked[actual.network]!=false)return null
        return actual
    }
    /** Provider-side guard may run off Main; it never reads callback maps or
     * issues bind/DNS. Fresh metadata also fences a queued same-Network change. */
    private fun observeActual():Observation? {
        if(closed.get()||!admitted())return null
        val network=connectivity.activeNetwork?:return null
        val links=connectivity.getLinkProperties(network)?:return null
        val capabilities=connectivity.getNetworkCapabilities(network)?:return null
        val own=Collections.list(checkNotNull(NetworkInterface.getNetworkInterfaces())).flatMap{Collections.list(it.inetAddresses)}
            .map(::numeric).distinct().sorted()
        require(own.isNotEmpty()&&own.size<=128)
        val routes=links.routes.filter{it.gateway==null||it.gateway?.isAnyLocalAddress==true}
            .map{it.destination}.filter{it.prefixLength>0}
            .map{numeric(it.address)+"/"+it.prefixLength}.distinct().sorted()
        require(routes.size<=128)
        val nat64=if(Build.VERSION.SDK_INT>=30)listOfNotNull(links.nat64Prefix).map{numeric(it.address)+"/"+it.prefixLength}else emptyList()
        // Private in-memory context only. DNS/Private-DNS/routing/capability
        // changes invalidate the old capture even if address lists are equal.
        val key=links.toString()+"|"+listOf(NetworkCapabilities.TRANSPORT_CELLULAR,NetworkCapabilities.TRANSPORT_WIFI,
            NetworkCapabilities.TRANSPORT_ETHERNET,NetworkCapabilities.TRANSPORT_VPN,NetworkCapabilities.TRANSPORT_BLUETOOTH)
            .filter{capabilities.hasTransport(it)}.joinToString()+"|"+
            listOf(NetworkCapabilities.NET_CAPABILITY_NOT_RESTRICTED,NetworkCapabilities.NET_CAPABILITY_TRUSTED,
                NetworkCapabilities.NET_CAPABILITY_NOT_VPN).map{capabilities.hasCapability(it)}.joinToString()
        if(connectivity.activeNetwork!=network)return null
        return Observation(network,own,routes,nat64,key)
    }
    private fun lose(network:Network){
        check(Looper.myLooper()==main.looper)
        val old=synchronized(lock){
            if(observed?.network!=network)return
            observed=null
            captures.toList()
        }
        old.forEach{it.invalidate()}
    }
    private fun refresh(){
        check(Looper.myLooper()==main.looper)
        val next=runCatching(::observe).getOrNull()
        val old=synchronized(lock){
            if(observed==next)emptyList() else{
                check(epoch<Long.MAX_VALUE){"Network observation epoch exhausted"}
                epoch++;observed=next;captures.toList()
            }
        }
        old.forEach{it.invalidate()}
        offerCapture()
    }
    private fun start(){
        check(Looper.myLooper()==main.looper)
        if(started)return
        check(!closed.get())
        connectivity.registerDefaultNetworkCallback(defaults,main)
        try {
            val request = NetworkRequest.Builder()
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                request.clearCapabilities()
            } else {
                // Remove the default capability filters on Android 9 and 10.
                request.removeCapability(NetworkCapabilities.NET_CAPABILITY_NOT_RESTRICTED)
                    .removeCapability(NetworkCapabilities.NET_CAPABILITY_TRUSTED)
                    .removeCapability(NetworkCapabilities.NET_CAPABILITY_NOT_VPN)
            }
            connectivity.registerNetworkCallback(request.build(), interfaces, main)
        } catch (failure: Exception) {
            connectivity.unregisterNetworkCallback(defaults)
            throw failure
        }
        started=true
    }
    fun capture():Capture {
        check(Looper.myLooper()!=main.looper){"Network capture must not wait on control Main"}
        val answer=requestCapture()
        try{return answer.get(5,TimeUnit.SECONDS)}
        catch(failure:Exception){answer.cancel(false);throw failure}
    }
    fun requestCapture():CompletableFuture<Capture>{
        val answer=CompletableFuture<Capture>()
        if(!main.post{
            try{
                check(!closed.get()&&admitted()&&(pending==null||pending?.isDone==true)){"Outbound capture is unavailable"}
                pending=answer;start();refresh();offerCapture()
            }catch(failure:Exception){if(pending===answer)pending=null;answer.completeExceptionally(failure)}
        })answer.completeExceptionally(IllegalStateException("Network observer is unavailable"))
        return answer
    }
    /** Only the execution owner calls this after actual native Closed or
     * no-admission. Invalidating a capture alone never authorizes its release. */
    fun release(capture:Capture){capture.invalidate();synchronized(lock){captures.remove(capture)}}
    private fun offerCapture(){
        check(Looper.myLooper()==main.looper)
        val answer=pending?:return
        if(answer.isDone){pending=null;return}
        if(closed.get()||!admitted()){pending=null;answer.completeExceptionally(IllegalStateException("Outbound capture was cancelled"));return}
        val selected=synchronized(lock){observed}?:return
        val capture=synchronized(lock){
            check(captures.isEmpty()&&epoch>0){"Outbound capture was already issued"}
            Capture(epoch,selected.network,JSONObject().put("network_epoch",epoch)
                .put("own_addresses",JSONArray(selected.own)).put("on_link",JSONArray(selected.onLink)).put("nat64",JSONArray(selected.nat64)).toString(),selected.signature())
                .also{captures.add(it)}
        }
        pending=null
        if(!answer.complete(capture))release(capture)
    }
    internal inner class Capture(val networkEpoch:Long,private val network:Network,private val snapshot:String,private val signature:String):WorkerEgressPlatform {
        private val gate=EgressCaptureGate{!closed.get()&&admitted()}
        override fun current():Boolean=gate.current
        private fun actualCurrent():Boolean {
            if(!gate.current)return false
            val matches=runCatching{observeActual()?.let{it.network==network&&it.signature()==signature}==true}.getOrDefault(false)
            if(!matches)gate.invalidate()
            return matches&&gate.current
        }
        override fun readCapture():String=if(actualCurrent())snapshot else ""
        override fun installRevoker(revoker:WorkerEgressRevoker):Boolean=gate.install{revoker.revoke()}
        override fun bindSocket(descriptor:Int):Boolean=runCatching{
            check(actualCurrent())
            ParcelFileDescriptor.fromFd(descriptor).use{network.bindSocket(it.fileDescriptor)}
            check(actualCurrent());true
        }.getOrDefault(false)
        override fun resolveName(ascii:String):List<String> = runCatching{
            check(actualCurrent())
            require(ascii.length in 1..255&&ascii.all{it in 'a'..'z'||it in 'A'..'Z'||it in '0'..'9'||it=='-'||it=='.'})
            val addresses=network.getAllByName(ascii)
            require(addresses.isNotEmpty()&&addresses.size<=64)
            val values=addresses.map{checkNotNull(it.hostAddress)} // scoped forms are rejected natively, never stripped.
            check(actualCurrent());values
        }.getOrDefault(emptyList())
        internal fun invalidate(){gate.invalidate()}
        override fun toString()="WorkerEgressCapture(private)"
    }
    fun revokeAll(){
        synchronized(lock){captures.toList()}.forEach{it.invalidate()}
        if(Looper.myLooper()==main.looper)offerCapture() else main.post{offerCapture()}
    }
    override fun close(){
        if(!closed.compareAndSet(false,true))return
        revokeAll()
        val cleanup={
            if(started){runCatching{connectivity.unregisterNetworkCallback(defaults)};runCatching{connectivity.unregisterNetworkCallback(interfaces)};started=false}
            synchronized(lock){observed=null}
            pending?.completeExceptionally(IllegalStateException("Outbound capture was closed"));pending=null;blocked.clear()
        }
        if(Looper.myLooper()==main.looper)cleanup() else main.post{cleanup()}
    }
}
