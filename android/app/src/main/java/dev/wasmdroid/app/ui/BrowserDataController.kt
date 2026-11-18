package dev.wasmdroid.app.ui

import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import org.json.JSONObject

internal interface BrowserDataBackend {
    val current: Boolean
    suspend fun state(): String
    suspend fun erase(revision: Long, profileToken: String): String
}
internal enum class BrowserDataStatus { Available, InUse, Clearing, Interrupted, Unsupported }
internal enum class BrowserDataProblem { Unavailable, Changed, InUse, Unsupported, Failed }

internal data class BrowserDataSnapshot(
    val revision: Long, val profileToken: String, val activeViews: Int,
    val status: BrowserDataStatus, val pending: Boolean, val cleared: Boolean,
) {
    val canErase: Boolean get() = status in setOf(BrowserDataStatus.Available, BrowserDataStatus.Interrupted)
    override fun toString() = "BrowserDataSnapshot(private)"
    companion object {
        fun parse(raw: String): BrowserDataSnapshot {
            require(raw.toByteArray(Charsets.UTF_8).size <= 16 * 1024)
            val json = JSONObject(raw)
            val revision = json.getLong("revision").also { require(it >= 0) }
            val token = json.getString("profile_token").also { require(it.matches(Regex("wasmdroid-[0-9a-f]{64}"))) }
            val active = json.getInt("active_views").also { require(it in 0..64) }
            val pending = json.getBoolean("pending_erasure")
            val outcome = json.getString("status").also { require(it in setOf("preserved", "cleared", "clearing", "failed", "interrupted")) }
            require(pending == (outcome in setOf("clearing", "failed", "interrupted")))
            val capabilities = json.getJSONObject("capabilities")
            val supported = capabilities.getBoolean("isolated_profiles") && capabilities.getBoolean("profile_data_erasure")
            val status = when {
                !supported -> BrowserDataStatus.Unsupported
                active > 0 -> BrowserDataStatus.InUse
                outcome == "clearing" -> BrowserDataStatus.Clearing
                pending -> BrowserDataStatus.Interrupted
                else -> BrowserDataStatus.Available
            }
            return BrowserDataSnapshot(revision, token, active, status, pending, outcome == "cleared")
        }
    }
}

internal class BrowserEraseConfirmation(val revision: Long, val profileToken: String) {
    override fun toString() = "BrowserEraseConfirmation(private)"
}
internal data class BrowserDataState(
    val snapshot: BrowserDataSnapshot? = null, val reading: Boolean = false,
    val clearing: Boolean = false, val confirmation: BrowserEraseConfirmation? = null,
    val problem: BrowserDataProblem? = null, val acknowledged: Boolean = false,
) {
    override fun toString() = "BrowserDataState(private)"
}

/** Private, non-saveable view state. The common owner keeps the actual provider
 * future after this controller/view disappears. Only its real ACK permits the
 * success notice; a poll, timeout or local cancellation cannot manufacture it. */
internal class BrowserDataController(private val scope: CoroutineScope, private val backend: BrowserDataBackend) {
    private val mutable = MutableStateFlow(BrowserDataState())
    val state = mutable.asStateFlow()
    private var read: Job? = null
    private var erase: Job? = null
    private var serial = 0L
    private var closed = false
    private var acknowledgedToken: String? = null
    private var acknowledgedRevision: Long? = null

    fun refresh() {
        if (closed || mutable.value.reading) return
        if (!backend.current) { unavailable(); return }
        val epoch = ++serial
        mutable.update { it.copy(reading = true) }
        read = scope.launch {
            try {
                val frame = BrowserDataSnapshot.parse(backend.state())
                if (!valid(epoch)) return@launch
                if (!backend.current) { unavailable(); return@launch }
                val previous = mutable.value
                val confirmationChanged = previous.confirmation?.let { it.revision != frame.revision || it.profileToken != frame.profileToken } == true
                val identityChanged = previous.snapshot?.profileToken?.let { it != frame.profileToken } == true
                if (acknowledgedToken != null && acknowledgedToken != frame.profileToken) acknowledgedToken = null
                val acknowledged = acknowledgedToken == frame.profileToken && frame.cleared &&
                    acknowledgedRevision?.let { frame.revision > it } == true
                mutable.value = previous.copy(snapshot = frame, reading = false,
                    confirmation = if (confirmationChanged || identityChanged) null else previous.confirmation,
                    problem = when {
                        confirmationChanged || identityChanged -> BrowserDataProblem.Changed
                        previous.problem in setOf(BrowserDataProblem.Changed, BrowserDataProblem.Failed, BrowserDataProblem.Unsupported) -> previous.problem
                        else -> null
                    }, acknowledged = acknowledged)
            } catch (failure: Exception) {
                if (failure is CancellationException) throw failure
                if (valid(epoch)) unavailable()
            } finally { if (valid(epoch)) mutable.update { it.copy(reading = false) } }
        }
    }

    fun reload() {
        if (closed || mutable.value.clearing) return
        mutable.update { it.copy(problem = null, confirmation = null, acknowledged = false) }
        acknowledgedToken = null
        acknowledgedRevision = null
        refresh()
    }

    fun requestConfirmation() {
        val current = mutable.value
        if (closed || current.reading || current.clearing) return
        if (!backend.current) { unavailable(); return }
        if (current.problem != null && current.problem != BrowserDataProblem.Failed) return
        val snapshot = current.snapshot ?: return
        if (!snapshot.canErase) return
        mutable.update { it.copy(confirmation = BrowserEraseConfirmation(snapshot.revision, snapshot.profileToken), acknowledged = false, problem = null) }
    }

    fun dismissConfirmation() { if (!mutable.value.clearing) mutable.update { it.copy(confirmation = null) } }

    fun confirm(selection: BrowserEraseConfirmation) {
        val current = mutable.value
        if (current.confirmation !== selection) return
        val snapshot = current.snapshot ?: return
        if (closed || current.clearing || current.reading) return
        if (!backend.current) { unavailable(); return }
        if (selection.revision != snapshot.revision || selection.profileToken != snapshot.profileToken) {
            mutable.update { it.copy(confirmation = null, problem = BrowserDataProblem.Changed) }; return
        }
        if (!snapshot.canErase) { mutable.update { it.copy(confirmation = null, problem = BrowserDataProblem.InUse) }; return }
        read?.cancel(); ++serial
        acknowledgedToken = null
        acknowledgedRevision = null
        mutable.update { it.copy(clearing = true, reading = false, confirmation = null, problem = null, acknowledged = false) }
        erase = scope.launch {
            try {
                backend.erase(selection.revision, selection.profileToken)
                if (!closed) {
                    // The returned string can omit the private owner token.
                    // Reload exact current owner state before displaying success.
                    acknowledgedToken = selection.profileToken
                    acknowledgedRevision = selection.revision
                    read?.cancel(); ++serial
                    mutable.update { it.copy(clearing = false, reading = false, problem = null) }
                    refresh()
                }
            } catch (failure: Exception) {
                if (failure is CancellationException) throw failure
                if (!closed) {
                    read?.cancel(); ++serial
                    mutable.update { it.copy(clearing = false, reading = false, confirmation = null, problem = classify(failure)) }
                    refresh()
                }
            } finally { if (!closed) mutable.update { it.copy(clearing = false) } }
        }
    }

    fun disconnected() {
        if (closed) return
        ++serial; read?.cancel(); read = null
        unavailable()
    }
    fun close() { closed = true; ++serial; read?.cancel(); erase?.cancel(); acknowledgedToken = null; acknowledgedRevision = null }
    private fun valid(epoch: Long) = !closed && epoch == serial
    private fun unavailable() { mutable.update { it.copy(reading = false, confirmation = null, problem = BrowserDataProblem.Unavailable) } }
    private fun classify(failure: Throwable): BrowserDataProblem {
        var actual = failure; repeat(8) { actual.cause?.let { actual = it } }
        val message = actual.message.orEmpty()
        return when {
            message.contains("changed", true) || message.contains("stale", true) || message.contains("not found", true) -> BrowserDataProblem.Changed
            message.contains("unsupported", true) || message.contains("provider cannot", true) -> BrowserDataProblem.Unsupported
            message.contains("close embedded", true) || message.contains("active", true) || message.contains("already", true) -> BrowserDataProblem.InUse
            !backend.current || message.contains("closing", true) || message.contains("connection", true) -> BrowserDataProblem.Unavailable
            else -> BrowserDataProblem.Failed
        }
    }
}
