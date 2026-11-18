package dev.wasmdroid.app.ui

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import org.json.JSONObject

internal data class AutomaticStartSource(
    val id: String,
    val identity: String,
    val version: ULong,
    val name: String,
    val editable: Boolean,
    val startOnBoot: Boolean,
    val restartPolicy: AutomaticRestartPolicy,
) {
    override fun toString() = "AutomaticStartSource(private)"

    companion object {
        fun parse(raw: String): AutomaticStartSource {
            require(raw.toByteArray(Charsets.UTF_8).size <= 4096)
            val value = JSONObject(raw)
            val version = value.getLong("version")
            require(version >= 0)
            return AutomaticStartSource(
                value.getString("id"), value.getString("identity"), version.toULong(),
                value.getString("name"), value.get("editable") as Boolean,
                value.get("start_on_boot") as Boolean,
                AutomaticRestartPolicy.parse(value.get("restart_policy")),
            )
        }
    }
}

internal interface AutomaticStartBackend {
    val current: Boolean
    suspend fun source(id: String): AutomaticStartSource
    suspend fun save(source: AutomaticStartSource, startOnBoot: Boolean, restartPolicy: AutomaticRestartPolicy)
}

internal data class AutomaticStartState(
    val id: String,
    val epoch: Long,
    val source: AutomaticStartSource? = null,
    val startOnBoot: Boolean = false,
    val restartPolicy: AutomaticRestartPolicy = AutomaticRestartPolicy.Never,
    val loading: Boolean = false,
    val saving: Boolean = false,
    val failed: Boolean = false,
) {
    override fun toString() = "AutomaticStartState(private)"
}

internal class AutomaticStartController(
    private val scope: CoroutineScope,
    private val capture: suspend () -> AutomaticStartBackend,
    private val refresh: suspend (AutomaticStartBackend) -> Unit,
) {
    private val draft = MutableStateFlow<AutomaticStartState?>(null)
    val state = draft.asStateFlow()
    private var epoch = 0L
    private var task: Job? = null
    private var backend: AutomaticStartBackend? = null

    fun available() = backend?.current == true

    fun edit(id: String) {
        if (draft.value?.saving == true) return
        task?.cancel()
        backend = null
        val pending = AutomaticStartState(id, ++epoch, loading = true)
        draft.value = pending
        task = scope.launch {
            try {
                val owner = capture()
                check(owner.current)
                val source = owner.source(id)
                check(owner.current && source.id == id)
                if (draft.value?.epoch != pending.epoch) return@launch
                backend = owner
                draft.value = pending.copy(source = source, startOnBoot = source.startOnBoot,
                    restartPolicy = source.restartPolicy, loading = false)
            } catch (failure: CancellationException) {
                throw failure
            } catch (_: Exception) {
                if (draft.value?.epoch == pending.epoch) draft.value = pending.copy(loading = false, failed = true)
            }
        }
    }

    fun reload(epoch: Long) {
        draft.value?.takeIf { it.epoch == epoch }?.let { edit(it.id) }
    }

    fun dismiss(epoch: Long) {
        val current = draft.value ?: return
        if (current.epoch != epoch || current.saving) return
        task?.cancel()
        backend = null
        draft.value = null
    }

    private fun editable(epoch: Long) = draft.value?.takeIf {
        it.epoch == epoch && !it.saving && !it.failed && it.source?.editable == true && available()
    }

    fun boot(epoch: Long, enabled: Boolean) {
        editable(epoch)?.let { draft.value = it.copy(startOnBoot = enabled) }
    }

    fun restart(epoch: Long, policy: AutomaticRestartPolicy) {
        editable(epoch)?.let { draft.value = it.copy(restartPolicy = policy) }
    }

    fun save(epoch: Long) {
        val current = editable(epoch) ?: return
        val owner = backend ?: return
        val source = current.source ?: return
        draft.value = current.copy(saving = true)
        task = scope.launch {
            try {
                owner.save(source, current.startOnBoot, current.restartPolicy)
            } catch (failure: CancellationException) {
                throw failure
            } catch (_: Exception) {
                draft.value = current.copy(failed = true)
                return@launch
            }
            backend = null
            draft.value = null
            if (owner.current) {
                try {
                    refresh(owner)
                } catch (failure: CancellationException) {
                    throw failure
                } catch (_: Exception) {
                    // The save already committed. Refresh can converge independently.
                }
            }
        }
    }
}
