package dev.wasmdroid.app

import android.os.Bundle
import android.os.Handler
import android.os.HandlerThread
import androidx.test.platform.app.InstrumentationRegistry
import dev.wasmdroid.app.engine.LanMonitor
import dev.wasmdroid.app.engine.LanNetworkCapture
import dev.wasmdroid.app.engine.LanSelectionLedger
import dev.wasmdroid.app.engine.LanSharingCoordinator
import dev.wasmdroid.app.engine.LanStartIntent
import dev.wasmdroid.app.engine.LanWifiCandidate
import dev.wasmdroid.app.engine.WorkerProtocol
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import java.util.concurrent.CompletableFuture
import java.util.concurrent.ExecutionException
import java.util.concurrent.TimeUnit

/** Protocol ordering on a real actor, with real ledger-issued captures. Worker
 * acknowledgements are controlled inputs, not evidence of Wi-Fi or guest I/O. */
class LanSharingCoordinatorTest {
    @Test fun defaultSharingWaitsForAdmissionAndExplicitStopRemainsOffForTheGeneration() = Fixture().use { f ->
        f.permission(false)
        f.actor { f.owner.readyDefault(f.endpoint) }
        assertEquals(0, f.commands(WorkerProtocol.LAN_ENABLE))

        f.permission(true)
        val start = f.take(WorkerProtocol.LAN_ENABLE)
        start.answer.complete(start.active())
        f.flush()
        val off = f.actor { f.owner.disable(f.endpoint, start.epoch) }
        val cleanup = f.take(WorkerProtocol.LAN_DISABLE)
        cleanup.answer.complete(cleanup.closed())
        off.get(3, TimeUnit.SECONDS)
        f.permission(true)
        assertEquals(1, f.commands(WorkerProtocol.LAN_ENABLE))
        assertEquals("off", f.status(f.endpoint).getString("phase"))

        val next = f.endpoint.copy(generation = "generation-2")
        f.actor {
            f.owner.ended(f.endpoint.id, f.endpoint.generation)
            f.current = next
            f.owner.readyDefault(next)
        }
        assertEquals(next, f.take(WorkerProtocol.LAN_ENABLE).endpoint)
    }

    @Test fun ambiguousWifiWaitsAndNetworkLossNeverSwitchesAnActiveDefaultGrant() = Fixture().use { f ->
        f.wifi(8, "10.1.2.4")
        f.actor { f.owner.readyDefault(f.endpoint) }
        assertEquals(0, f.commands(WorkerProtocol.LAN_ENABLE))

        f.loseWifi(8)
        val start = f.take(WorkerProtocol.LAN_ENABLE)
        assertEquals(7L, start.network)
        start.answer.complete(start.active())
        f.flush()
        f.wifi(9, "192.168.3.4")
        f.loseWifi(7)
        val cleanup = f.take(WorkerProtocol.LAN_DISABLE)
        assertEquals("stopping", f.status(f.endpoint).getString("phase"))
        cleanup.answer.complete(cleanup.closed())
        f.flush()
        assertEquals("off", f.status(f.endpoint).getString("phase"))
        assertEquals(1, f.commands(WorkerProtocol.LAN_ENABLE))
    }

    @Test fun dualUrlsUseAttestedAddressesAndAnIpv6DriftClosesTheGrant() = Fixture().use { f ->
        f.ipv6("fd80::2", 64)
        val (answer, request) = f.begin(f.endpoint)
        assertEquals(64, request.scope.getInt("ipv6_prefix_length"))
        assertTrue(LanSelectionLedger.sameIpv6Address("fd80::2", request.scope.getString("ipv6_address")))
        request.answer.complete(JSONObject(request.active()).put("ipv6_address", "fd80::2").toString())
        val started = JSONObject(answer.get(3, TimeUnit.SECONDS))
        assertEquals("http://192.168.1.8:49123/web", started.getString("url"))
        assertEquals("http://[fd80::2]:49123/web", started.getString("ipv6_url"))
        val status = f.actor { f.owner.status(f.endpoint) }
        val query = f.take(WorkerProtocol.LAN_STATUS)
        query.answer.complete(JSONObject(query.active()).put("ipv6_address", "fd80::3").toString())
        val stopped = JSONObject(status.get(3, TimeUnit.SECONDS))
        assertFalse(stopped.has("url"))
        assertFalse(stopped.has("ipv6_url"))
        val cleanup = f.take(WorkerProtocol.LAN_DISABLE)
        cleanup.answer.complete(cleanup.closed(failed = true))
        f.flush()
        assertEquals("off", f.status(f.endpoint).getString("phase"))
    }

    @Test fun cancelledIntentBeforeQueuedDispatchPreventsAnyWorkerSend() = Fixture().use { f ->
        f.deferEnableDispatch = true
        val intent = LanStartIntent()
        val (answer, request) = f.begin(f.endpoint, intent)
        val capture = f.capture
        assertFalse(request.dispatched)
        intent.cancel()
        f.flush()
        assertFalse(capture.locallyActive)
        assertEquals("off", f.status(f.endpoint).getString("phase"))
        f.actor { request.dispatch() }
        failed(answer)
        assertFalse(request.dispatched)
        assertEquals(0, f.commands(WorkerProtocol.LAN_DISABLE))
        assertEquals(0, f.revocations(f.endpoint))
        assertFalse(f.status(f.endpoint).has("url"))
    }

    @Test fun cancelledIntentAfterDispatchSurvivesAbandonedObserverUntilExactCleanup() = Fixture().use { f ->
        val intent = LanStartIntent()
        val (answer, request) = f.begin(f.endpoint, intent)
        assertTrue(request.dispatched)
        assertTrue(answer.cancel(false))
        assertTrue(f.capture.locallyActive)
        assertEquals(0, f.commands(WorkerProtocol.LAN_DISABLE))
        intent.cancel()
        f.flush()
        val cleanup = f.take(WorkerProtocol.LAN_DISABLE)
        assertTrue(f.capture.locallyActive)
        request.answer.complete(request.active())
        f.flush()
        val stopping = f.status(f.endpoint)
        assertEquals("stopping", stopping.getString("phase"))
        assertFalse(stopping.has("url"))
        assertTrue(f.capture.locallyActive)
        cleanup.answer.complete(cleanup.closed())
        f.flush()
        assertFalse(f.capture.locallyActive)
        assertTrue(answer.isCancelled)
        assertEquals("off", f.status(f.endpoint).getString("phase"))
    }

    @Test fun offDuringEnableCannotBeUndoneByLateActiveAcknowledgement() = Fixture().use { f ->
        val (enable, request) = f.begin(f.endpoint)
        val off = f.actor { f.owner.disable(f.endpoint, request.epoch) }
        val cleanup = f.take(WorkerProtocol.LAN_DISABLE)
        assertTrue(f.revocations(f.endpoint) > 0)
        assertTrue(f.capture.locallyActive)
        request.answer.complete(request.active())
        f.flush()
        failed(enable)
        assertFalse(off.isDone)
        val stopping = f.status(f.endpoint)
        assertEquals("stopping", stopping.getString("phase"))
        assertFalse(stopping.has("url"))
        assertTrue(f.capture.locallyActive)
        cleanup.answer.complete(cleanup.closed())
        off.get(3, TimeUnit.SECONDS)
        f.flush()
        assertEquals("off", f.status(f.endpoint).getString("phase"))
        assertFalse(f.capture.locallyActive)
        assertEquals(1, f.commands(WorkerProtocol.LAN_DISABLE))
    }

    @Test fun statusCoalescesPrivateReceiptDespiteCancelledOrForgedObservers() = Fixture().use { f ->
        f.active(f.endpoint)
        val first = f.actor { f.owner.status(f.endpoint) }
        val request = f.take(WorkerProtocol.LAN_STATUS)
        val second = f.actor { f.owner.status(f.endpoint) }
        assertTrue(first.cancel(false))
        assertTrue(second.complete("caller supplied value"))
        val retained = f.actor { f.owner.status(f.endpoint) }
        assertFalse(request.answer.isDone)
        assertFalse(retained.isDone)
        assertEquals(1, f.commands(WorkerProtocol.LAN_STATUS))
        assertTrue(f.capture.locallyActive)
        request.answer.complete(request.closed(failed = true))
        val result = JSONObject(retained.get(3, TimeUnit.SECONDS))
        f.flush()
        assertEquals("off", result.getString("phase"))
        assertTrue(result.getBoolean("failed"))
        assertEquals(request.epoch, result.getLong("last_grant_epoch"))
        assertFalse(result.has("url"))
        assertFalse(f.capture.locallyActive)
        assertTrue(f.status(f.endpoint).getBoolean("failed"))
    }

    @Test fun activeStatusInFlightBeforeOffCannotRestoreUrlOrInventFailure() = Fixture().use { f ->
        f.active(f.endpoint)
        val status = f.actor { f.owner.status(f.endpoint) }
        val query = f.take(WorkerProtocol.LAN_STATUS)
        val off = f.actor { f.owner.disable(f.endpoint, query.epoch) }
        val cleanup = f.take(WorkerProtocol.LAN_DISABLE)
        query.answer.complete(query.active())
        val result = JSONObject(status.get(3, TimeUnit.SECONDS))
        assertEquals("stopping", result.getString("phase"))
        assertFalse(result.getBoolean("failed"))
        assertFalse(result.has("url"))
        assertFalse(off.isDone)
        assertTrue(f.capture.locallyActive)
        assertEquals(1, f.commands(WorkerProtocol.LAN_DISABLE))
        cleanup.answer.complete(cleanup.closed())
        off.get(3, TimeUnit.SECONDS)
        f.flush()
        assertFalse(f.status(f.endpoint).getBoolean("failed"))
    }

    @Test fun observedOldGenerationDeathFencesItsLateAckFromReplacementGrant() = Fixture().use { f ->
        val (oldAnswer, old) = f.begin(f.endpoint)
        val oldCapture = f.capture
        val replacement = f.endpoint.copy(generation = "generation-2")
        // This input represents the owner's observed death + durable terminal
        // publication gate; it does not simulate or establish actual OS death.
        f.actor {
            f.owner.ended(f.endpoint.id, f.endpoint.generation)
            f.current = replacement
        }
        val (newAnswer, fresh) = f.begin(replacement)
        assertTrue(fresh.epoch > old.epoch)
        assertFalse(oldCapture.locallyActive)
        old.answer.complete(old.active())
        f.flush()
        failed(oldAnswer)
        assertFalse(newAnswer.isDone)
        assertEquals("starting", f.status(replacement).getString("phase"))
        fresh.answer.complete(fresh.active())
        val active = JSONObject(newAnswer.get(3, TimeUnit.SECONDS))
        assertEquals("active", active.getString("phase"))
        assertEquals(fresh.epoch, active.getLong("grant_epoch"))
        assertTrue(active.has("url"))
        assertTrue(f.capture.locallyActive)
        assertEquals(0, f.commands(WorkerProtocol.LAN_DISABLE))
    }

    @Test fun mismatchedCleanupEpochRemainsOwnedUntilExactReceipt() = Fixture().use { f ->
        val active = f.active(f.endpoint)
        val first = f.actor { f.owner.disable(f.endpoint, active.epoch) }
        val bad = f.take(WorkerProtocol.LAN_DISABLE)
        bad.answer.complete(JSONObject(bad.closed()).put("grant_epoch", bad.epoch + 1).toString())
        failed(first)
        // A native revoking status is not a cleanup receipt and must not allow
        // a new grant to bypass the still-owned previous scope.
        val status = f.actor { f.owner.status(f.endpoint) }
        val query = f.take(WorkerProtocol.LAN_STATUS)
        query.answer.complete(JSONObject().put("grant_epoch", bad.epoch).put("phase", "revoking").toString())
        val state = JSONObject(status.get(3, TimeUnit.SECONDS))
        assertTrue(state.getBoolean("failed"))
        assertFalse(state.has("url"))
        assertTrue(f.capture.locallyActive)
        assertTrue(f.actor { runCatching { f.owner.enable(f.endpoint, f.selection(), { true }) }.isFailure })
        val retry = f.actor { f.owner.disable(f.endpoint, active.epoch) }
        val exact = f.take(WorkerProtocol.LAN_DISABLE)
        exact.answer.complete(exact.closed(failed = true))
        retry.get(3, TimeUnit.SECONDS)
        f.flush()
        assertFalse(f.capture.locallyActive)
        assertEquals("off", f.status(f.endpoint).getString("phase"))
        assertTrue(f.status(f.endpoint).getBoolean("failed"))
    }

    private class Request(val endpoint:LanSharingCoordinator.Endpoint, val what:Int, bundle:Bundle,
        private val admission:()->Boolean) {
        val scope = JSONObject(requireNotNull(bundle.getString(WorkerProtocol.LAN_SCOPE)))
        val network = bundle.getLong(WorkerProtocol.LAN_NETWORK)
        val answer = CompletableFuture<String>()
        var dispatched = false
            private set
        fun dispatch() {
            check(!dispatched)
            try { check(admission()) { "Private worker send admission changed" }; dispatched = true }
            catch (failure:Exception) { answer.completeExceptionally(failure) }
        }
        val epoch:Long get() = scope.getLong("grant_epoch")
        fun active():String = JSONObject(scope.toString()).put("phase", "active").put("host_port", 49123).toString()
        fun closed(failed:Boolean = false):String = JSONObject().put("phase", "closed")
            .put("grant_epoch", epoch).put("failed", failed).put("failed_connections", if (failed) 1 else 0).toString()
    }

    private class Monitor : LanMonitor {
        val ledger = LanSelectionLedger(true).apply {
            available(7)
            capabilities(7, wifi = true, notVpn = true, suspended = false, cellular = false)
            links(7, "wlan-fixture", listOf("192.168.1.8"))
            blocked(7, false)
        }
        var permission = true
        val captures = mutableListOf<LanNetworkCapture>()
        override fun start() = Unit
        override fun candidates():List<LanWifiCandidate> = if (permission) ledger.candidates() else emptyList()
        override fun capture(candidate:LanWifiCandidate):LanNetworkCapture = ledger.capture(candidate) { permission }
            .also { captures.add(it) }
        override fun close() = ledger.close()
    }

    private class Fixture : AutoCloseable {
        private val thread = HandlerThread("lan-coordinator-regression").apply { start() }
        private val handler = Handler(thread.looper)
        private val monitor = Monitor()
        private val requests = mutableListOf<Request>()
        private val consumed = mutableSetOf<Request>()
        private val revocations = mutableListOf<Pair<LanSharingCoordinator.Endpoint, String>>()
        private var selectionToken:String? = null
        var deferEnableDispatch = false
        val endpoint = LanSharingCoordinator.Endpoint("app-fixture", "generation-1", 8080, "/web")
        var current = endpoint
        val owner = actor {
            LanSharingCoordinator(InstrumentationRegistry.getInstrumentation().targetContext, handler,
                { true }, { it == current },
                { endpoint, what, bundle, admission -> Request(endpoint, what, bundle, admission).also {
                    requests.add(it)
                    if (what != WorkerProtocol.LAN_ENABLE || !deferEnableDispatch) it.dispatch()
                }.answer },
                { endpoint, bundle -> revocations.add(endpoint to requireNotNull(bundle.getString(WorkerProtocol.LAN_SCOPE))) },
                { monitor })
        }
        val capture:LanNetworkCapture get() = actor { monitor.captures.last() }
        fun <T> actor(action:()->T):T {
            val result = CompletableFuture<T>()
            check(handler.post { try { result.complete(action()) } catch (failure:Throwable) { result.completeExceptionally(failure) } })
            return result.get(3, TimeUnit.SECONDS)
        }
        fun flush() { actor { Unit }; actor { Unit } }
        fun permission(allowed: Boolean) = actor {
            monitor.permission = allowed
            owner.admissionChanged()
        }
        fun wifi(handle: Long, address: String) = actor {
            monitor.ledger.available(handle)
            monitor.ledger.capabilities(handle, wifi = true, notVpn = true, suspended = false, cellular = false)
            monitor.ledger.links(handle, "wlan-fixture-$handle", listOf(address))
            monitor.ledger.blocked(handle, false)
            owner.admissionChanged()
        }
        fun loseWifi(handle: Long) = actor {
            monitor.ledger.lost(handle)
            owner.admissionChanged()
        }
        fun ipv6(address: String, prefixLength: Int) = actor {
            monitor.ledger.links(
                7, "wlan-fixture", listOf("192.168.1.8"), LanSelectionLedger.OnLinkIpv6(address, prefixLength),
            )
        }
        fun selection():String {
            selectionToken?.let { return it }
            val snapshot = owner.status(current)
            check(snapshot.isDone) { "Selection must be read before any grant is pending" }
            return JSONObject(snapshot.getNow(null)).getJSONArray("candidates").getJSONObject(0)
                .getString("selection_token").also { selectionToken = it }
        }
        fun status(endpoint:LanSharingCoordinator.Endpoint):JSONObject {
            val result = actor { owner.status(endpoint) }
            return JSONObject(result.get(3, TimeUnit.SECONDS))
        }
        fun begin(endpoint:LanSharingCoordinator.Endpoint,intent:LanStartIntent = LanStartIntent()):Pair<CompletableFuture<String>, Request> {
            val answer = actor { owner.enable(endpoint, selection(), { true }, intent) }
            val request = take(WorkerProtocol.LAN_ENABLE)
            assertEquals(7L, request.network)
            return answer to request
        }
        fun active(endpoint:LanSharingCoordinator.Endpoint):Request {
            val (answer, request) = begin(endpoint)
            request.answer.complete(request.active())
            assertEquals("active", JSONObject(answer.get(3, TimeUnit.SECONDS)).getString("phase"))
            return request
        }
        fun take(what:Int):Request = actor {
            requests.firstOrNull { it.what == what && it !in consumed }
                ?.also { consumed.add(it) } ?: error("Expected worker command $what")
        }
        fun commands(what:Int):Int = actor { requests.count { it.what == what } }
        fun revocations(endpoint:LanSharingCoordinator.Endpoint):Int = actor { revocations.count { it.first == endpoint } }
        override fun close() {
            try {
                actor {
                    // Test teardown supplies the explicit terminal-gate input,
                    // avoiding a fabricated native cleanup acknowledgement.
                    requests.map { it.endpoint }.distinct().forEach { owner.ended(it.id, it.generation) }
                    owner.close()
                }
                flush()
            } finally {
                thread.quitSafely()
                thread.join(3000)
                check(!thread.isAlive) { "LAN regression actor did not terminate" }
            }
        }
    }

    companion object {
        private fun failed(answer:CompletableFuture<String>) {
            try { answer.get(3, TimeUnit.SECONDS); fail("Expected rejected acknowledgement") }
            catch (expected:ExecutionException) { assertTrue(expected.cause is IllegalStateException) }
        }
    }
}
