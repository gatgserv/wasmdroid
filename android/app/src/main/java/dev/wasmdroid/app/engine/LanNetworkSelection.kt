package dev.wasmdroid.app.engine

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.net.ConnectivityManager
import android.net.LinkProperties
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.os.Build
import android.os.Handler
import android.os.ParcelFileDescriptor
import android.system.OsConstants
import dev.wasmdroid.app.LocalNetworkPermission
import java.io.FileDescriptor
import java.net.Inet4Address
import java.net.Inet6Address
import java.net.InetAddress
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicBoolean

/** Process-private callback observation, not a persistent grant or an incoming
 * intent/JSON capability. Native parent grant epochs remain separately owned. */
internal class LanWifiCandidate internal constructor(
    internal val issuer:Any,
    val networkHandle:Long,
    val interfaceEpoch:Long,
    val privateAddress:String,
    val blockedStatusObserved:Boolean,
    val ipv6Address:String? = null,
    val ipv6PrefixLength:Int? = null,
) {
    override fun toString():String = "LanWifiCandidate(<private interface scope>)"
}

/** Invalidation fences admission; it is not a transport-cleanup receipt. The
 * actual owner must revoke forwarding and await its real terminal completion. */
internal class LanNetworkCapture internal constructor(
    val candidate:LanWifiCandidate,
    private val current:()->Boolean,
    private val release:()->Unit,
) : AutoCloseable {
    private val active = AtomicBoolean(true)
    private val lock = Any()
    private var callback:(()->Unit)? = null
    private var observerRegistered = false
    internal val locallyActive:Boolean get() = active.get()
    val valid:Boolean get() = locallyActive && current()
    fun onInvalidated(action:()->Unit) {
        val notify = synchronized(lock) {
            check(!observerRegistered) { "LAN capture already has an invalidation observer" }
            observerRegistered = true
            if (active.get()) { callback = action; false } else true
        }
        if (notify) action()
    }
    internal fun invalidate():(()->Unit)? = synchronized(lock) {
        if (!active.getAndSet(false)) null else callback.also { callback = null }
    }
    override fun close() = release()
    override fun toString():String = "LanNetworkCapture(<private process capability>)"
}

/** Pure ordered-observation ledger. It issues reference-checked capabilities;
 * copied values, another process/helper's candidate and stale epochs fail. */
internal class LanSelectionLedger(private val blockedObservationRequired:Boolean) {
    data class OnLinkIpv6(val address:String, val prefixLength:Int)

    private class Record(var epoch:Long) {
        var eligibleCapability:Boolean? = null
        var interfaceName:String? = null
        var addresses:List<String>? = null
        var ipv6:OnLinkIpv6? = null
        var blocked:Boolean? = null
        var candidates:List<LanWifiCandidate> = emptyList()
    }
    private val lock = Any()
    private val issuer = Any()
    private val records = linkedMapOf<Long,Record>()
    private val captures = linkedSetOf<LanNetworkCapture>()
    private var epoch = 0L
    private var closed = false

    private fun nextEpoch():Long {
        check(epoch < Long.MAX_VALUE) { "LAN interface epoch exhausted" }
        return ++epoch
    }
    private fun revokeLocked(handle:Long?):List<()->Unit> {
        val selected = captures.filter { handle == null || it.candidate.networkHandle == handle }
        selected.forEach { captures.remove(it) }
        return selected.mapNotNull { it.invalidate() }
    }
    private fun deliver(callbacks:List<()->Unit>) {
        // One observer cannot prevent other captures from being invalidated.
        // The owner must reconcile valid flags and await actual native cleanup.
        callbacks.forEach { runCatching(it) }
    }
    private fun rebuild(handle:Long, record:Record) {
        val ready = record.eligibleCapability == true && record.interfaceName != null &&
            record.addresses != null && (!blockedObservationRequired || record.blocked == false)
        record.candidates = if (ready) record.addresses.orEmpty().map {
            LanWifiCandidate(issuer, handle, record.epoch, it, blockedObservationRequired,
                record.ipv6?.address, record.ipv6?.prefixLength)
        } else emptyList()
    }
    fun available(handle:Long) {
        require(handle != 0L) { "LAN network handle is unavailable" }
        val callbacks = synchronized(lock) {
            if (closed) return
            check(handle in records || records.size < 8) { "LAN network observation limit exceeded" }
            val revoked = revokeLocked(handle)
            records[handle] = Record(nextEpoch())
            revoked
        }
        deliver(callbacks)
    }
    fun capabilities(handle:Long, wifi:Boolean, notVpn:Boolean, suspended:Boolean, cellular:Boolean) = update(handle) { record ->
        val eligible = wifi && notVpn && !suspended && !cellular
        if (record.eligibleCapability == eligible) false else { record.eligibleCapability = eligible; true }
    }
    fun links(handle:Long, interfaceName:String?, addresses:List<String>, ipv6:OnLinkIpv6? = null) = update(handle) { record ->
        val name = interfaceName?.takeIf { it.length in 1..64 && it.none(Char::isISOControl) }
        val normalized = if (addresses.size <= 8) addresses.mapNotNull(::privateIpv4).distinct().sorted() else emptyList()
        val ipv6Address = ipv6?.let { wifiIpv6(it.address, it.prefixLength) }
        if (record.interfaceName == name && record.addresses == normalized && record.ipv6 == ipv6Address) false
        else { record.interfaceName = name; record.addresses = normalized; record.ipv6 = ipv6Address; true }
    }
    fun blocked(handle:Long, blocked:Boolean) {
        if (!blockedObservationRequired) return // API28 explicitly Unobserved.
        update(handle) { record ->
            if (record.blocked == blocked) false else { record.blocked = blocked; true }
        }
    }
    private fun update(handle:Long, change:(Record)->Boolean) {
        val callbacks = synchronized(lock) {
            if (closed) return
            val record = records[handle] ?: return // No authority before onAvailable.
            if (!change(record)) return
            val revoked = revokeLocked(handle)
            record.epoch = nextEpoch()
            rebuild(handle, record)
            revoked
        }
        deliver(callbacks)
    }
    fun lost(handle:Long) {
        val callbacks = synchronized(lock) {
            records.remove(handle)
            revokeLocked(handle)
        }
        deliver(callbacks)
    }
    fun candidates():List<LanWifiCandidate> = synchronized(lock) {
        if (closed) emptyList() else records.values.flatMap { it.candidates }
    }
    private fun currentCandidate(candidate:LanWifiCandidate):Boolean = !closed && candidate.issuer === issuer &&
        records[candidate.networkHandle]?.candidates?.any { it === candidate } == true
    fun capture(candidate:LanWifiCandidate, admitted:()->Boolean):LanNetworkCapture {
        check(admitted()) { "LAN sharing is not admitted" }
        val result = synchronized(lock) {
            check(currentCandidate(candidate) && captures.size < 64) { "LAN network selection changed" }
            lateinit var result:LanNetworkCapture
            result = LanNetworkCapture(candidate, { isCurrent(result, admitted) }, { release(result) })
            captures.add(result)
            result
        }
        check(isCurrent(result, admitted)) { "LAN sharing admission changed" }
        return result
    }
    fun isCurrent(capture:LanNetworkCapture, admitted:()->Boolean):Boolean {
        if (!runCatching(admitted).getOrDefault(false)) { invalidateAll(); return false }
        return synchronized(lock) { capture.locallyActive && capture in captures && currentCandidate(capture.candidate) }
    }
    fun <T> withCurrent(capture:LanNetworkCapture, admitted:()->Boolean, action:()->T):T {
        check(isCurrent(capture, admitted)) { "LAN network selection changed" }
        // Never hold the ledger lock across a platform call. Callback loss or
        // policy revocation must be able to invalidate during socket binding.
        val result = action()
        check(isCurrent(capture, admitted)) { "LAN network selection changed during binding" }
        return result
    }
    private fun release(capture:LanNetworkCapture) {
        val callback = synchronized(lock) { captures.remove(capture); capture.invalidate() }
        callback?.invoke()
    }
    fun invalidateAll() {
        val callbacks = synchronized(lock) { revokeLocked(null) }
        deliver(callbacks)
    }
    fun unavailable() {
        val callbacks = synchronized(lock) { records.clear(); revokeLocked(null) }
        deliver(callbacks)
    }
    fun close() {
        val callbacks = synchronized(lock) { closed = true; records.clear(); revokeLocked(null) }
        deliver(callbacks)
    }
    companion object {
        internal fun ipv6UsableForListening(flags: Int): Boolean =
            flags and OsConstants.IFA_F_DADFAILED == 0 &&
                (flags and OsConstants.IFA_F_TENTATIVE == 0 || flags and OsConstants.IFA_F_OPTIMISTIC != 0)

        internal fun sameIpv6Address(left:String?, right:String?):Boolean {
            if (left == null || right == null) return left == right
            val parsed = ipv6Literal(left) ?: return false
            return parsed == ipv6Literal(right)
        }

        internal fun wifiIpv6(value:String, prefixLength:Int):OnLinkIpv6? {
            val address = ipv6Literal(value) ?: return null
            val first = address.address[0].toInt() and 0xff
            if (prefixLength !in 64..127 || (first and 0xe0 != 0x20 && first and 0xfe != 0xfc)) return null
            return OnLinkIpv6(checkNotNull(address.hostAddress), prefixLength)
        }

        private fun ipv6Literal(value:String):Inet6Address? {
            if (value.length !in 2..39 || ':' !in value || value.any { it !in "0123456789abcdefABCDEF:" }) return null
            return runCatching { InetAddress.getByName(value) as? Inet6Address }.getOrNull()
        }

        /** Parse a literal without DNS or platform-default address fallback. */
        internal fun privateIpv4(value:String):String? {
            if (value.length !in 7..15) return null
            val parts = value.split('.')
            if (parts.size != 4) return null
            val bytes = parts.map { part ->
                if (part.isEmpty() || part.any { it !in '0'..'9' }) return null
                val byte = part.toIntOrNull()?.takeIf { it in 0..255 && it.toString() == part } ?: return null
                byte
            }
            return value.takeIf { bytes[0] == 10 || bytes[0] == 172 && bytes[1] in 16..31 || bytes[0] == 192 && bytes[1] == 168 }
        }
    }
}

internal object LanSocketBinding {
    /** fromFd duplicates; never adopt/close/detach the native owner's descriptor.
     * Real Network.bindSocket requires the owner's unconnected socket. */
    fun bindDuplicate(fd:Int, platformBind:(FileDescriptor)->Unit) {
        require(fd >= 0) { "LAN socket descriptor is unavailable" }
        ParcelFileDescriptor.fromFd(fd).use { duplicate -> platformBind(duplicate.fileDescriptor) }
    }
}

/** Explicitly started process-local monitor. It requests no network, reads no
 * SSID/location and never grants LAN permission. A callback selects only its
 * ordered arguments: https://developer.android.com/reference/android/net/ConnectivityManager.NetworkCallback */
internal class LanNetworkSelection(
    context:Context,
    private val callbackHandler:Handler,
    private val admitted:()->Boolean,
    private val onChanged:()->Unit,
) : AutoCloseable {
    private val context = context.applicationContext
    private val manager = this.context.getSystemService(ConnectivityManager::class.java)
    private val lifecycle = Any()
    private val networks = ConcurrentHashMap<Long,Network>()
    private val ledger = LanSelectionLedger(Build.VERSION.SDK_INT >= 29)
    @Volatile private var started = false
    @Volatile private var closed = false
    private val callback = object:ConnectivityManager.NetworkCallback() {
        override fun onAvailable(network:Network) = observe {
            networks[network.networkHandle] = network
            ledger.available(network.networkHandle)
        }
        override fun onCapabilitiesChanged(network:Network, capabilities:NetworkCapabilities) = observe {
            ledger.capabilities(network.networkHandle,
                capabilities.hasTransport(NetworkCapabilities.TRANSPORT_WIFI),
                capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_VPN) &&
                    !capabilities.hasTransport(NetworkCapabilities.TRANSPORT_VPN),
                !capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_SUSPENDED),
                capabilities.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR))
        }
        override fun onLinkPropertiesChanged(network:Network, properties:LinkProperties) = observe {
            val addresses = properties.linkAddresses.mapNotNull { (it.address as? Inet4Address)?.hostAddress }
            val ipv6 = onLinkIpv6(properties)
            ledger.links(network.networkHandle, properties.interfaceName, addresses, ipv6)
        }
        override fun onBlockedStatusChanged(network:Network, blocked:Boolean) = observe {
            ledger.blocked(network.networkHandle, blocked)
        }
        override fun onLosing(network:Network, maxMsToLive:Int) = onLost(network)
        override fun onLost(network:Network) = observe {
            ledger.lost(network.networkHandle); networks.remove(network.networkHandle, network)
        }
        override fun onUnavailable() = observe { ledger.unavailable(); networks.clear() }
    }
    private fun onLinkIpv6(properties:LinkProperties):LanSelectionLedger.OnLinkIpv6? {
        val onLink = properties.routes.filter {
            it.getInterface() == properties.interfaceName &&
                (it.gateway == null || it.gateway?.isAnyLocalAddress == true)
        }.map { it.destination }
        return properties.linkAddresses.mapNotNull { link ->
            val address = link.address as? Inet6Address ?: return@mapNotNull null
            val flags = link.flags
            if (!LanSelectionLedger.ipv6UsableForListening(flags)) return@mapNotNull null
            val literal = address.hostAddress?.substringBefore('%') ?: return@mapNotNull null
            val prefix = onLink.filter { it.prefixLength in 64..127 && it.contains(address) }
                .maxOfOrNull { it.prefixLength } ?: return@mapNotNull null
            val scoped = LanSelectionLedger.wifiIpv6(literal, prefix) ?: return@mapNotNull null
            scoped to flags
        // Assigned optimistic and deprecated addresses can receive traffic.
        // Prefer an ordinary preferred address when one is available.
        }.minWithOrNull(compareBy<Pair<LanSelectionLedger.OnLinkIpv6,Int>> {
            it.second and (OsConstants.IFA_F_DEPRECATED or OsConstants.IFA_F_OPTIMISTIC) != 0
        }.thenBy {
            !it.first.address.startsWith("fc") && !it.first.address.startsWith("fd")
        }.thenBy { it.second and OsConstants.IFA_F_TEMPORARY != 0 }.thenBy { it.first.address })?.first
    }
    private fun permission():Boolean = context.checkSelfPermission(Manifest.permission.ACCESS_NETWORK_STATE) == PackageManager.PERMISSION_GRANTED
    private fun admittedNow():Boolean = !closed && started && permission() &&
        LocalNetworkPermission.granted(context) && runCatching(admitted).getOrDefault(false)
    private fun observe(action:()->Unit) {
        if (closed) return
        try { action() } catch (_:RuntimeException) { close(); return }
        onChanged()
    }
    fun start() {
        synchronized(lifecycle) {
            check(!closed) { "LAN network monitor is closed" }
            if (started) return@synchronized
            check(permission()) { "LAN network observation permission is unavailable" }
            val service = requireNotNull(manager) { "LAN network observation is unavailable" }
            val request = NetworkRequest.Builder().addTransportType(NetworkCapabilities.TRANSPORT_WIFI)
                .addCapability(NetworkCapabilities.NET_CAPABILITY_NOT_VPN).build()
            try { service.registerNetworkCallback(request, callback, callbackHandler); started = true }
            catch (_:RuntimeException) {
                runCatching { service.unregisterNetworkCallback(callback) }
                closed = true; ledger.close(); networks.clear()
                error("LAN network observation is unavailable")
            }
        }
        // Initial callbacks can arrive before registerNetworkCallback returns.
        onChanged()
    }
    fun candidates():List<LanWifiCandidate> = if (admittedNow()) ledger.candidates() else emptyList()
    fun capture(candidate:LanWifiCandidate):LanNetworkCapture = ledger.capture(candidate, ::admittedNow)
    /** Parent-supplied values only select among actual local callback objects;
     * they do not create authority and are never restored from an Intent. */
    fun captureMatching(networkHandle:Long, privateAddress:String, ipv6Address:String?, ipv6PrefixLength:Int?):LanNetworkCapture = capture(
        candidates().singleOrNull { it.networkHandle == networkHandle && it.privateAddress == privateAddress &&
            LanSelectionLedger.sameIpv6Address(it.ipv6Address, ipv6Address) && it.ipv6PrefixLength == ipv6PrefixLength }
            ?: error("Selected Wi-Fi interface is no longer available"))
    fun isCurrent(capture:LanNetworkCapture):Boolean = ledger.isCurrent(capture, ::admittedNow)
    fun admissionChanged() { if (!admittedNow()) ledger.invalidateAll(); onChanged() }
    fun bind(capture:LanNetworkCapture, fd:Int) = ledger.withCurrent(capture, ::admittedNow) {
        val network = networks[capture.candidate.networkHandle] ?: error("Selected Wi-Fi network is unavailable")
        LanSocketBinding.bindDuplicate(fd) { network.bindSocket(it) }
    }
    override fun close() {
        synchronized(lifecycle) {
            if (closed) return
            closed = true
            ledger.close(); networks.clear()
            if (started) { started = false; runCatching { manager?.unregisterNetworkCallback(callback) } }
        }
        onChanged()
    }
}
