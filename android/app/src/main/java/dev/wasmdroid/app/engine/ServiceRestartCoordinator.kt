package dev.wasmdroid.app.engine

/** Actor-confined restart reservations. Timers never grant worker ownership.
 * The common owner must revalidate a ticket immediately before every native
 * replacement and worker reservation, and register only after reserving a Job. */
class ServiceRestartCoordinator(
    private val schedule: (Long, () -> Unit) -> Unit,
    private val current: (String) -> Record?,
    private val launch: (Ticket) -> Unit,
    private val changed: () -> Unit = {},
    private val failed: (Ticket, Throwable) -> Unit = { _, _ -> },
) {
    enum class Policy {
        NO, ON_FAILURE, ALWAYS;
        companion object {
            fun parse(value: String): Policy = when (value) {
                "no" -> NO
                "on_failure" -> ON_FAILURE
                "always" -> ALWAYS
                else -> throw IllegalArgumentException("Unsupported restart policy")
            }
        }
    }

    enum class State { CREATED, EXITED, DEAD }

    data class Record(
        val id: String,
        val version: ULong,
        val state: State,
        val policy: Policy,
        val managed: Boolean = false,
        val owned: Boolean = false,
        val authorityRevision: ULong = 0uL,
        val desiredRun: Boolean = false,
        val epochMatches: Boolean = false,
    )

    data class Completion(
        val record: Record,
        val generation: String,
        val exitCode: Int?,
        val hasError: Boolean,
        val ownerStopped: Boolean,
        val deathObserved: Boolean,
        val terminalCommitted: Boolean,
        val runningMillis: Long,
    )

    /** Token identity, original generation and terminal revision are immutable.
     * A Dead record is replaced through the existing versioned native API;
     * it is never changed back into a reusable identity. */
    class Ticket internal constructor(
        val sourceId: String,
        val id: String,
        val generation: String,
        val version: ULong,
        val state: State,
        val policy: Policy,
        val authorityRevision: ULong,
        val recovery: Boolean,
        internal val attempt: Int,
        internal val token: Any,
    )

    private val live = mutableMapOf<String, String>()
    private val attempts = mutableMapOf<String, Int>()
    private val pending = linkedMapOf<String, Ticket>()
    val pendingCount: Int get() = pending.size
    fun pendingTickets(): List<Ticket> = pending.values.toList()

    /** Manual starts reset backoff. Automatic starts carry it across a Dead
     * replacement. Call after the common owner has reserved the actual Job. */
    fun started(id: String, generation: String, ticket: Ticket? = null) {
        if (ticket == null) {
            cancel(id)
            attempts.remove(id)
        } else {
            check(isPending(ticket) && ticket.id == id) { "Restart reservation changed" }
            pending.remove(ticket.sourceId)
            attempts.remove(ticket.sourceId)
            attempts[id] = ticket.attempt
        }
        live[id] = generation
        changed()
    }

    /** Only observed process death AND a committed terminal record can reserve
     * a restart. Duplicate/late completion notifications cannot schedule twice. */
    fun completed(completion: Completion): Boolean {
        val record = completion.record
        if (!completion.deathObserved || !completion.terminalCommitted ||
            live[record.id] != completion.generation) return false
        live.remove(record.id)
        if (record.managed || !record.desiredRun || !record.epochMatches || completion.ownerStopped || record.state == State.CREATED ||
            record.policy == Policy.NO ||
            (record.policy == Policy.ON_FAILURE && record.state != State.DEAD &&
                completion.exitCode == 0 && !completion.hasError)) {
            attempts.remove(record.id)
            return false
        }
        val previous = if (completion.runningMillis >= STABLE_RUN_MILLIS) 0 else attempts[record.id] ?: 0
        val attempt = (previous + 1).coerceAtMost(MAX_BACKOFF_STEP)
        return enqueue(record,completion.generation,attempt,false)
    }

    /** Only native candidates obtained under the new exclusive common owner
     * enter here. Native recovery has committed interrupted terminal metadata;
     * eligibility includes desired-run, exact boot identity and policy/outcome. */
    fun recover(record:Record,generation:String):Boolean {
        if(record.managed||record.owned||!record.desiredRun||!record.epochMatches||record.policy==Policy.NO||
            record.id in live||pending.values.any{it.id==record.id})return false
        return enqueue(record,generation,1,true)
    }
    private fun enqueue(record:Record,generation:String,attempt:Int,recovery:Boolean):Boolean {
        val ticket = Ticket(record.id, record.id, generation, record.version,
            record.state, record.policy,record.authorityRevision,recovery,attempt,Any())
        pending[record.id] = ticket
        changed()
        try {
            schedule(backoffMillis(attempt)) { dispatch(ticket) }
        } catch (failure: Exception) {
            admissionFailed(ticket, failure)
        }
        return isPending(ticket)
    }

    /** Check on the actor immediately before native retry and again before
     * worker start. Includes both ownership/data fencing and revision/policy. */
    fun valid(ticket: Ticket, record: Record?): Boolean = isPending(ticket) && record != null &&
        record.id == ticket.id && record.version == ticket.version &&
        record.state == ticket.state && record.policy == ticket.policy &&
        record.authorityRevision==ticket.authorityRevision&&record.desiredRun&&record.epochMatches&&
        !record.managed && !record.owned

    fun isPending(ticket: Ticket): Boolean = pending[ticket.sourceId] === ticket

    /** Called only after the versioned Dead replacement commits. Keep its
     * reservation alive until the replacement Job is actually registered. */
    fun replaced(ticket: Ticket, replacement: Record): Ticket {
        check(isPending(ticket) && ticket.state == State.DEAD) { "Restart reservation changed" }
        check(replacement.state == State.CREATED && replacement.id != ticket.id &&
            replacement.policy == ticket.policy && replacement.desiredRun && replacement.epochMatches && !replacement.managed && !replacement.owned) {
            "Restart replacement differs from the admitted service"
        }
        val next = Ticket(ticket.sourceId, replacement.id, ticket.generation, replacement.version,
            replacement.state, replacement.policy,replacement.authorityRevision,ticket.recovery,ticket.attempt,ticket.token)
        pending[ticket.sourceId] = next
        return next
    }

    /** Cancel before manual stop/start, edit, data admission or removal. A
     * cancellation using either pre-replacement or current ID fences the timer. */
    fun cancel(id: String) {
        val removed = pending.filter { (source, ticket) -> source == id || ticket.id == id }.keys
        removed.forEach { pending.remove(it); attempts.remove(it) }
        attempts.remove(id)
        if (removed.isNotEmpty()) changed()
    }

    fun cancelAll() {
        val hadPending = pending.isNotEmpty()
        pending.clear()
        attempts.clear()
        if (hadPending) changed()
    }

    /** Failed ownership/capacity/native admission stays stopped and is reported;
     * a timer does not silently retry an unrelated or changed configuration. */
    fun admissionFailed(ticket: Ticket, failure: Throwable) {
        if (!isPending(ticket)) return
        pending.remove(ticket.sourceId)
        attempts.remove(ticket.sourceId)
        changed()
        failed(ticket, failure)
    }

    private fun dispatch(ticket: Ticket) {
        if (!isPending(ticket)) return
        try {
            check(valid(ticket, current(ticket.id))) { "Automatic restart admission changed" }
            // Keep the reservation until started()/admissionFailed(). A posted
            // start must not create an idle foreground-service lifetime gap.
            launch(ticket)
        } catch (failure: Exception) {
            admissionFailed(ticket, failure)
        }
    }

    companion object {
        // Technical anti-spin delay, not a retry-count product limit. A stable
        // minute resets the delay; repeated fast exits cap at one minute.
        const val STABLE_RUN_MILLIS = 60_000L
        private const val MAX_BACKOFF_STEP = 7
        fun backoffMillis(attempt: Int): Long =
            (1_000L shl (attempt.coerceIn(1, MAX_BACKOFF_STEP) - 1)).coerceAtMost(60_000L)
    }
}
