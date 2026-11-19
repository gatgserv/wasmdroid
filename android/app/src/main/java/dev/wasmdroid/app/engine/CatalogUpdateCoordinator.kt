package dev.wasmdroid.app.engine

import android.content.Context
import dev.wasmdroid.engine.EngineStorage
import dev.wasmdroid.engine.InstallControl
import dev.wasmdroid.engine.InstallStatus
import org.json.JSONObject
import java.io.FileNotFoundException
import java.util.UUID
import java.util.concurrent.CompletableFuture
import java.util.concurrent.Executor
import java.util.concurrent.atomic.AtomicReference

/** Owner-retained metadata operation. No network request is made by construction,
 * snapshot reads or missing configuration. Only refresh() explicitly fetches. */
internal class CatalogUpdateCoordinator(
    context: Context,
    private val storage: CompletableFuture<EngineStorage>,
    private val io: Executor,
    private val post: (() -> Unit) -> Boolean,
    private val admitted: () -> Boolean,
    private val finished: () -> Unit,
) {
    private data class Distribution(val json: String?, val invalid: Boolean = false)
    private data class Refresh(val generation: String, val control: InstallControl, val answer: CompletableFuture<String>)
    private val distribution = loadDistribution(context.applicationContext)
    private val active = AtomicReference<Refresh?>(null)
    private val outcome = AtomicReference<JSONObject?>(null)
    private var closing = false // actor only
    val idle: Boolean get() = active.get() == null

    /** Runs on IO: no mutable actor-owned state is consulted, and native calls
     * obtain independent connections/ownership leases. The config is immutable. */
    fun state(db: EngineStorage): String {
        val config = distribution.json
        val frame = JSONObject().put("configuration", when {
            distribution.invalid -> "invalid"
            config == null -> "disabled"
            else -> "configured"
        })
        frame.put("snapshot", if (config == null) JSONObject.NULL else {
            val snapshot = db.catalogSnapshot(config, now())
            if (snapshot == "null") JSONObject.NULL else JSONObject(snapshot)
        })
        val refresh = active.get()
        frame.put("refreshing", refresh != null)
        refresh?.let { frame.put("generation", it.generation) }
        outcome.get()?.let { frame.put("last_refresh", JSONObject(it.toString())) }
        return frame.toString()
    }

    /** Actor admission retains the native control until actual completion, even
     * when a ViewModel or caller future disappears. No retry/auto-fetch loop. */
    fun refresh(): CompletableFuture<String> {
        check(!closing && admitted()) { "Supervisor is closing" }
        val config = distribution.json ?: error(if (distribution.invalid)
            "Catalog update configuration is invalid" else "Catalog updates require owner-approved distribution configuration")
        check(active.get() == null) { "Catalog refresh is already active" }
        val operation = Refresh(UUID.randomUUID().toString(), InstallControl(), CompletableFuture())
        check(active.compareAndSet(null, operation)) { "Catalog refresh is already active" }
        outcome.set(null)
        try {
            CompletableFuture.supplyAsync({ storage.join().refreshCatalog(config, now(), operation.control) }, io)
                .whenComplete { value, failure ->
                    // The supervisor keeps its actor alive until idle is true.
                    post {
                        val status = runCatching { operation.control.status() }.getOrNull()
                        val terminal = if (failure == null) "updated" else if (status == InstallStatus.CANCELLED) "cancelled" else "failed"
                        outcome.set(JSONObject().put("generation", operation.generation).put("status", terminal))
                        active.compareAndSet(operation, null)
                        operation.control.close()
                        if (failure == null) operation.answer.complete(value)
                        else operation.answer.completeExceptionally(IllegalStateException(
                            if (terminal == "cancelled") "Catalog refresh cancelled" else "Catalog refresh failed; the last accepted snapshot was preserved"))
                        finished()
                    }
                }
        } catch (failure: Exception) {
            active.compareAndSet(operation, null)
            operation.control.close()
            operation.answer.completeExceptionally(failure)
            finished()
        }
        return operation.answer
    }

    /** Actor only. Stale generations cannot cancel a replacement operation. */
    fun cancel(generation: String): Boolean {
        val operation = active.get()?.takeIf { it.generation == generation } ?: return false
        return operation.control.cancel()
    }

    /** Actor only. Shutdown waits for the actual callback through idle. */
    fun close() {
        closing = true
        active.get()?.let { runCatching { it.control.cancel() } }
    }

    /** IO-safe exact selection. A pin has no recipe or URL authority. The native
     * accepted snapshot supplies the recipe immediately before controlled install. */
    fun packageForInstall(db: EngineStorage, pinJson: String): String {
        require(pinJson.toByteArray(Charsets.UTF_8).size <= 1024) { "Catalog selection exceeds input limit" }
        val pin = JSONObject(pinJson)
        val config = distribution.json ?: error("Catalog updates are not configured")
        return db.catalogPackageForInstall(config, pin.getString("sequence").toULong(),
            pin.getString("catalog_digest"), pin.getString("package_id"), now())
    }

    private fun now(): ULong = (System.currentTimeMillis().coerceAtLeast(0) / 1000).toULong()

    companion object {
        /** Optional PUBLIC trust config in the owner-controlled APK asset overlay.
         * It must contain no private signing key or credentials. No production
         * config asset is shipped. Untrusted imports/preferences cannot set it. */
        private fun loadDistribution(context: Context): Distribution = try {
            context.assets.open("catalog-updates.json").use { input ->
                val limit = 16 * 1024
                val bytes = ByteArray(limit + 1)
                var size = 0
                while (size < bytes.size) {
                    val count = input.read(bytes, size, bytes.size - size)
                    if (count < 0) break
                    if (count == 0) continue
                    size += count
                }
                require(size <= limit) { "Catalog configuration exceeds limit" }
                val json = bytes.copyOf(size).toString(Charsets.UTF_8)
                JSONObject(json)
                Distribution(json)
            }
        } catch (_: FileNotFoundException) {
            Distribution(null)
        } catch (_: Exception) {
            Distribution(null, invalid = true)
        }
    }
}
