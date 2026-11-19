package dev.wasmdroid.app.engine

import org.json.JSONArray
import java.net.URI
import java.util.UUID
import java.util.concurrent.atomic.AtomicBoolean

/** Only the common owner publishes actual ready worker mappings. A UI URL or
 * persisted port never grants a browser session. Revocation is synchronous,
 * before worker stop/release; view destruction is then dispatched by the UI.
 * This is lifecycle authority, NOT a complete browser transport sandbox. */
class BrowserSessionAuthority {
    data class Mapping(val instancePort: Int, val hostPort: Int)
    private data class Generation(val token: String, val profileId: String, val ports: List<Mapping>, val leases: MutableSet<BrowserSessionLease> = linkedSetOf())
    private val lock = Any()
    private val running = mutableMapOf<String, Generation>()
    private var closed = false

    fun publish(instanceId: String, generation: String, actualPortsJson: String, profileId: String) {
        identity(instanceId); UUID.fromString(generation)
        BrowserProfileLedger.profileIdentity(profileId)
        require(actualPortsJson.toByteArray(Charsets.UTF_8).size <= 16 * 1024) { "Browser mapping exceeds input limit" }
        val ports = JSONArray(actualPortsJson)
        require(ports.length() <= 64) { "Too many browser mappings" }
        val mappings = (0 until ports.length()).map { ports.getJSONObject(it) }
            .filter { it.getString("protocol") == "tcp" }
            .map { Mapping(it.getInt("instance_port"), it.getInt("host_port")) }
        require(mappings.all { it.instancePort in 1..65535 && it.hostPort in 1..65535 }) { "Browser mapping is not allocated" }
        require(mappings.map { it.instancePort }.distinct().size == mappings.size &&
            mappings.map { it.hostPort }.distinct().size == mappings.size) { "Ambiguous browser mapping" }
        synchronized(lock) {
            check(!closed) { "Browser authority is closed" }
            val previous = running[instanceId]
            if (previous != null) {
                check(previous.token == generation && previous.profileId == profileId && previous.ports == mappings) { "Revoke the previous browser generation before replacement" }
                return
            }
            check(running.values.none { active -> active.ports.any { first -> mappings.any { it.hostPort == first.hostPort } } }) { "Browser port is owned by another generation" }
            running[instanceId] = Generation(generation, profileId, mappings)
        }
    }

    fun acquire(instanceId: String, expectedGeneration: String, instancePort: Int, path: String,
                requireCompleteNetworkIsolation: Boolean = false): BrowserSessionLease {
        identity(instanceId)
        if (requireCompleteNetworkIsolation) throw UnsupportedOperationException(
            "Complete browser egress and port-reuse isolation require an enforced browser transport gateway")
        require(path.length <= 4096 && path.startsWith('/') && !path.startsWith("//") &&
            !path.contains('\\') && path.none { it.isISOControl() }) { "Browser path must be local" }
        return synchronized(lock) {
            check(!closed) { "Browser authority is closed" }
            val current = running[instanceId] ?: error("Application has no ready browser generation")
            check(current.token == expectedGeneration) { "Browser generation changed" }
            check(running.values.sumOf { it.leases.size } < 64) { "Too many active browser sessions" }
            val binding = current.ports.singleOrNull { it.instancePort == instancePort } ?: error("Application port is not published")
            val url = "http://127.0.0.1:${binding.hostPort}$path"
            val uri = URI(url)
            require(uri.rawAuthority == "127.0.0.1:${binding.hostPort}" && uri.userInfo == null) { "Invalid local browser target" }
            BrowserSessionLease(instanceId, current.profileId, current.token, instancePort, binding.hostPort, url) { lease ->
                synchronized(lock) { current.leases.remove(lease) }
            }.also { current.leases.add(it) }
        }
    }

    /** A stale worker callback cannot revoke a newer session of the same app. */
    fun revoke(instanceId: String, expectedGeneration: String) {
        val callbacks = synchronized(lock) {
            val current = running[instanceId]?.takeIf { it.token == expectedGeneration } ?: return
            running.remove(instanceId)
            current.leases.mapNotNull { it.invalidate() }.also { current.leases.clear() }
        }
        callbacks.forEach { runCatching(it) }
    }

    fun close() {
        val callbacks = synchronized(lock) {
            closed = true
            running.values.flatMap { it.leases }.mapNotNull { it.invalidate() }.also { running.clear() }
        }
        callbacks.forEach { runCatching(it) }
    }

    companion object {
        internal fun identity(value: String) {
            require(value.length in 1..128 && value.all { it.isLetterOrDigit() && it.code < 128 || it == '_' || it == '-' }) { "Invalid browser instance identity" }
        }
    }
}

/** Opaque owner-issued capability. Callbacks can check valid from any thread;
 * they remain incomplete WebView egress checks even when this returns true. */
class BrowserSessionLease internal constructor(
    val instanceId: String,
    /** Private native identity, stable across same-spec runtime replacements.
     * Never taken from labels, URLs, a UI target or a caller-provided alias. */
    val profileId: String,
    val generation: String,
    val instancePort: Int,
    val hostPort: Int,
    val url: String,
    private val release: (BrowserSessionLease) -> Unit,
) : AutoCloseable {
    private val active = AtomicBoolean(true)
    private val lock = Any()
    private var revoked: (() -> Unit)? = null
    val valid: Boolean get() = active.get()
    val completeNetworkIsolation: Boolean get() = false
    val exclusiveTransportGateway: Boolean get() = false

    fun onRevoked(callback: () -> Unit) {
        val notify = synchronized(lock) {
            check(revoked == null) { "Browser revocation callback already registered" }
            if (active.get()) { revoked = callback; false } else true
        }
        if (notify) callback()
    }

    internal fun invalidate(): (() -> Unit)? = synchronized(lock) {
        if (!active.getAndSet(false)) return@synchronized null
        revoked.also { revoked = null }
    }

    fun permitsOrdinaryRequest(value: String): Boolean {
        if (!valid) return false
        return runCatching { URI(value).let { it.scheme == "http" && it.host == "127.0.0.1" && it.port == hostPort && it.userInfo == null } }.getOrDefault(false)
    }

    override fun close() {
        invalidate()
        release(this)
    }
}
