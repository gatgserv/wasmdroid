package dev.wasmdroid.app

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.os.IBinder
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import dev.wasmdroid.app.engine.EngineService
import dev.wasmdroid.app.engine.ServiceSupervisor
import dev.wasmdroid.app.ui.ServiceConfigurationDraft
import dev.wasmdroid.app.ui.WorkerSample
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.util.UUID
import java.util.concurrent.CompletableFuture
import java.util.concurrent.TimeUnit

/** Explicit device test with a real registry package and two test-owned workers. */
@RunWith(AndroidJUnit4::class)
class ServiceTimeoutAcceptanceTest {
    @Test
    fun finiteTimeoutStopsOnlyItsWorkerAndEmptyTimeoutKeepsStatsAvailable() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val ready = CompletableFuture<ServiceSupervisor>()
        val connection = object : ServiceConnection {
            override fun onServiceConnected(name: ComponentName?, binder: IBinder?) {
                ready.complete((binder as EngineService.LocalBinder).engine)
            }

            override fun onServiceDisconnected(name: ComponentName?) = Unit
        }
        assertTrue(context.bindService(Intent(context, EngineService::class.java), connection, Context.BIND_AUTO_CREATE))
        val created = mutableListOf<String>()
        var supervisor: ServiceSupervisor? = null
        try {
            val engine = ready.get(20, TimeUnit.SECONDS)
            supervisor = engine
            val reference = "wasmer.io/python/python@3.13.20"
            engine.pull(reference).get(300, TimeUnit.SECONDS)
            val base = JSONObject()
                .put("artifact_ref", reference)
                .put("command", "python")
                .put("stdin_open", true)
                .put("args", JSONArray().put("-u").put("-c").put("""
                    import sys
                    assert sys.stdin.readline() == "release\n"
                    print("WASMDROID_TIMEOUT_RELEASED", flush=True)
                """.trimIndent()))
            val draft = ServiceConfigurationDraft.fromSpecification(base.toString())

            fun create(timeout: String): String {
                val id = "test-timeout-${UUID.randomUUID()}"
                engine.create(id, draft.copy(timeoutSeconds = timeout).specification()).get(20, TimeUnit.SECONDS)
                created += id
                return id
            }

            val unlimited = create("")
            val limited = create("3")
            fun persistedTimeout(id: String): JSONObject =
                JSONObject(engine.inspect(id).get(10, TimeUnit.SECONDS)).getJSONObject("spec").getJSONObject("resources")

            assertTrue(persistedTimeout(unlimited).isNull("timeout_seconds"))
            assertEquals(3L, persistedTimeout(limited).getLong("timeout_seconds"))

            val unlimitedRun = JSONObject(engine.start(unlimited).get(180, TimeUnit.SECONDS)).getString("generation")
            val before = WorkerSample.parse(engine.workerStats(unlimited).get(10, TimeUnit.SECONDS))
            assertEquals(unlimitedRun, before.generation)
            assertTrue(requireNotNull(before.residentBytes) > 0)
            assertTrue(requireNotNull(before.cpuNs) > 0)

            val limitedRun = JSONObject(engine.start(limited).get(180, TimeUnit.SECONDS)).getString("generation")
            val timedOut = JSONObject(engine.waitForExit(limited).get(30, TimeUnit.SECONDS))
            assertEquals(limitedRun, timedOut.getString("generation"))
            val stopped = timedOut.getJSONObject("instance")
            assertTrue(stopped.getString("state") in setOf("exited", "dead"))
            assertEquals("Service execution timed out", stopped.getString("last_error"))

            val after = WorkerSample.parse(engine.workerStats(unlimited).get(10, TimeUnit.SECONDS))
            assertEquals(unlimitedRun, after.generation)
            assertTrue(requireNotNull(after.residentBytes) > 0)
            assertTrue(requireNotNull(after.cpuPercent(before)) >= 0)
            engine.writeStdin(unlimited, unlimitedRun, "release\n".toByteArray()).get(10, TimeUnit.SECONDS)
            val completion = JSONObject(engine.waitForExit(unlimited).get(30, TimeUnit.SECONDS))
            val result = completion.getJSONObject("guest_result")
            assertEquals(0, result.getInt("exit_code"))
            assertTrue(result.getString("stdout").contains("WASMDROID_TIMEOUT_RELEASED"))
        } finally {
            try {
                val cleanupFailures = created.asReversed().mapNotNull { id ->
                    runCatching {
                        supervisor?.stop(id, 0)?.get(20, TimeUnit.SECONDS)
                        supervisor?.remove(id)?.get(10, TimeUnit.SECONDS)
                    }.exceptionOrNull()
                }
                if (cleanupFailures.isNotEmpty()) {
                    val failure = AssertionError("Could not remove test-owned worker records")
                    cleanupFailures.forEach(failure::addSuppressed)
                    throw failure
                }
            } finally {
                context.unbindService(connection)
            }
        }
    }
}
