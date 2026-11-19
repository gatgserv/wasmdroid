package dev.wasmdroid.app.engine

import java.security.MessageDigest
import java.util.UUID
import java.util.concurrent.atomic.AtomicBoolean

/** Process-wide lease/erasure exclusion, independent of provider APIs. Persisted
 * pending erasure blocks reacquisition after process interruption. Only another
 * explicit erasure request may resume it; preservation is the default. */
class BrowserProfileLedger(private val persistence: Persistence) {
    data class Record(val revision: Long = 0, val eraseGeneration: String? = null, val outcome: String = "preserved")
    data class Snapshot(val revision: Long, val activeViews: Int, val eraseGeneration: String?, val status: String)
    /** Opaque identity: no generated copy/component API can manufacture another
     * ticket. Completion also requires this exact issuer-retained reference. */
    class EraseTicket internal constructor(val profileId: String, val generation: String, val revision: Long)
    interface Persistence {
        fun read(profileId: String): Record?
        /** Must durably finish or throw before destructive provider work begins. */
        fun write(profileId: String, record: Record)
    }
    private val lock = Any()
    private val active = mutableMapOf<String, MutableMap<String,String>>()
    private val erasing = mutableMapOf<String, EraseTicket>()

    fun snapshot(profileId: String): Snapshot = synchronized(lock) {
        profileIdentity(profileId)
        val record = record(profileId)
        val status = if (record.eraseGeneration != null && record.outcome == "clearing" && erasing[profileId]?.generation != record.eraseGeneration) "interrupted" else record.outcome
        Snapshot(record.revision, active[profileId]?.size ?: 0, record.eraseGeneration, status)
    }

    fun acquire(profileId: String, generation: String): BrowserProfileLease = synchronized(lock) {
        profileIdentity(profileId)
        UUID.fromString(generation)
        val current = record(profileId)
        check(current.eraseGeneration == null && profileId !in erasing) { "Browser data erasure must finish before reopening this profile" }
        check(active[profileId].orEmpty().values.all { it == generation }) { "Wait for the previous browser generation to finish closing" }
        check(active.values.sumOf { it.size } < 64) { "Too many active browser profiles" }
        val token = UUID.randomUUID().toString()
        active.getOrPut(profileId) { linkedMapOf() }[token] = generation
        BrowserProfileLease(profileId, generation, profileName(profileId), current.revision) {
            synchronized(lock) {
                active[profileId]?.let { views -> views.remove(token); if (views.isEmpty()) active.remove(profileId) }
            }
        }
    }

    /** Requires explicit caller intent and the exact displayed profile revision.
     * A lost process may retry a pending request, but active duplicate requests
     * and living view leases cannot start overlapping destructive work. */
    fun beginErase(profileId: String, expectedRevision: Long): EraseTicket = synchronized(lock) {
        profileIdentity(profileId)
        val current = record(profileId)
        check(current.revision == expectedRevision) { "Browser profile revision changed" }
        check(active[profileId].isNullOrEmpty()) { "Close embedded views before clearing browser data" }
        check(profileId !in erasing) { "Browser data erasure is already active" }
        check(erasing.size < 16) { "Too many active browser data operations" }
        val revision = Math.addExact(current.revision, 1)
        val generation = UUID.randomUUID().toString()
        val ticket = EraseTicket(profileId, generation, revision)
        persistence.write(profileId, Record(revision, generation, "clearing"))
        erasing[profileId] = ticket
        ticket
    }

    /** Called only on the provider's completion acknowledgement. A stale callback
     * cannot unblock a later erase or publish a false completion receipt. */
    fun completed(ticket: EraseTicket): Boolean = synchronized(lock) {
        val current = record(ticket.profileId)
        if (erasing[ticket.profileId] !== ticket || current.eraseGeneration != ticket.generation || current.revision != ticket.revision) return@synchronized false
        persistence.write(ticket.profileId, Record(Math.addExact(current.revision, 1), null, "cleared"))
        erasing.remove(ticket.profileId)
        true
    }

    /** Failure keeps the durable pending fence. Data may already be partly
     * cleared; never relabel that state as preserved or claim successful wipe. */
    fun failed(ticket: EraseTicket): Boolean = synchronized(lock) {
        if (erasing[ticket.profileId] !== ticket) return@synchronized false
        val current = record(ticket.profileId)
        if (current.eraseGeneration != ticket.generation || current.revision != ticket.revision) {
            erasing.remove(ticket.profileId)
            return@synchronized false
        }
        try { persistence.write(ticket.profileId, current.copy(outcome = "failed")) }
        finally { erasing.remove(ticket.profileId) }
        true
    }

    private fun record(id: String): Record = (persistence.read(id) ?: Record()).also {
        check(it.revision >= 0 && it.outcome in setOf("preserved", "clearing", "cleared", "failed")) { "Invalid browser profile receipt" }
        it.eraseGeneration?.let(UUID::fromString)
        check((it.eraseGeneration == null) == (it.outcome in setOf("preserved", "cleared"))) { "Browser profile receipt is inconsistent" }
    }

    companion object {
        /** Native-owned namespaces: existing instance IDs retain exact old
         * hashes; newly allocated identities cannot collide with any legacy ID. */
        fun profileIdentity(value: String) {
            if (value.startsWith("browser:")) {
                val sequence = value.removePrefix("browser:")
                require(sequence.matches(Regex("[1-9][0-9]{0,18}")) && sequence.toLongOrNull()?.let { it > 0 } == true) { "Invalid private browser identity" }
            } else BrowserSessionAuthority.identity(value)
        }
        fun profileName(profileId: String): String {
            profileIdentity(profileId)
            return "wasmdroid-" + MessageDigest.getInstance("SHA-256").digest(profileId.toByteArray(Charsets.UTF_8))
                .joinToString("") { "%02x".format(it.toInt() and 0xff) }
        }
    }
}

class BrowserProfileLease internal constructor(
    val profileId: String,
    val generation: String,
    val profileName: String,
    val revision: Long,
    private val release: () -> Unit,
) : AutoCloseable {
    private val closed = AtomicBoolean(false)
    val valid: Boolean get() = !closed.get()
    override fun close() { if (closed.compareAndSet(false, true)) release() }
}
