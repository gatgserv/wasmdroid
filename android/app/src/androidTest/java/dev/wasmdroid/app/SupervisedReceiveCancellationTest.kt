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
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.io.DataInputStream
import java.net.ConnectException
import java.net.InetSocketAddress
import java.net.Socket
import java.net.SocketException
import java.net.SocketTimeoutException
import java.util.UUID
import java.util.concurrent.CompletableFuture
import java.util.concurrent.TimeUnit

/** Live test of cancellation while the guest is blocked in a default TCP receive. */
@RunWith(AndroidJUnit4::class)
class SupervisedReceiveCancellationTest {
    @Test
    fun stopClosesIdleAcceptedStreamAndItsListenerAfterFormerReceiveDeadline() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val ready = CompletableFuture<ServiceSupervisor>()
        val connection = object : ServiceConnection {
            override fun onServiceConnected(name: ComponentName?, binder: IBinder?) {
                val engine = (binder as? EngineService.LocalBinder)?.engine
                if (engine == null) ready.completeExceptionally(IllegalStateException("Owner binding unavailable"))
                else ready.complete(engine)
            }

            override fun onServiceDisconnected(name: ComponentName?) = Unit
        }
        val intent = Intent(context, EngineService::class.java).setAction(EngineService.ACTION_SHORTCUT_BIND)
        assertTrue(context.bindService(intent, connection, Context.BIND_AUTO_CREATE))

        val id = "test-receive-stop-${UUID.randomUUID()}"
        var supervisor: ServiceSupervisor? = null
        var created = false
        var stopped = false
        var testFailure: Throwable? = null
        try {
            val engine = ready.get(30, TimeUnit.SECONDS)
            supervisor = engine
            val reference = "wasmer.io/python/python@3.13.20"
            engine.pull(reference).get(300, TimeUnit.SECONDS)
            val script = """
                import socket

                with socket.socket() as listener:
                    listener.bind(("127.0.0.1", 28082))
                    listener.listen(8)
                    peer, _ = listener.accept()
                    with peer:
                        assert peer.recv(1) == b"!"
                        peer.sendall(b"recv")
                        peer.recv(1)
            """.trimIndent()
            val spec = JSONObject()
                .put("artifact_ref", reference)
                .put("command", "python")
                .put("args", JSONArray().put("-u").put("-c").put(script))
                .put("ports", JSONArray().put(JSONObject()
                    .put("instance_port", 28082).put("host_port", 0).put("protocol", "tcp")))
                .put("network", "loopback")
            engine.create(id, spec.toString()).get(20, TimeUnit.SECONDS)
            created = true

            val running = JSONObject(engine.start(id).get(180, TimeUnit.SECONDS))
            val generation = running.getString("generation")
            val hostPort = running.getJSONArray("ports").getJSONObject(0).getInt("host_port")
            assertTrue(hostPort in 1..65535)
            val address = InetSocketAddress("127.0.0.1", hostPort)

            Socket().use { client ->
                client.connect(address, 5_000)
                client.soTimeout = 15_000
                client.getOutputStream().write('!'.code)
                val input = DataInputStream(client.getInputStream())
                val marker = ByteArray(4)
                input.readFully(marker)
                assertEquals("recv", String(marker, Charsets.US_ASCII))

                // The Java client's deadline bounds observation only. The guest
                // receives no more bytes and sets no receive timeout.
                client.soTimeout = 32_000
                assertThrows(SocketTimeoutException::class.java) { input.read() }
                val active = JSONObject(engine.terminalSession(id).get(10, TimeUnit.SECONDS))
                assertEquals(generation, active.getString("generation"))

                val receipt = JSONObject(engine.stop(id).get(15, TimeUnit.SECONDS))
                assertEquals(generation, receipt.getString("generation"))
                val instance = receipt.getJSONObject("instance")
                assertEquals(id, instance.getString("id"))
                assertTrue(instance.getString("state") in setOf("exited", "dead"))
                stopped = true

                client.soTimeout = 3_000
                try {
                    assertEquals("Established stream must close when the worker stops", -1, input.read())
                } catch (failure: SocketException) {
                    // Forced worker death can reset the peer instead of sending FIN.
                    assertTrue("Test client must not have been closed locally: $failure", !client.isClosed)
                }
            }
            assertThrows(ConnectException::class.java) {
                Socket().use { it.connect(address, 1_000) }
            }
        } catch (failure: Throwable) {
            testFailure = failure
            throw failure
        } finally {
            try {
                if (created) {
                    val engine = requireNotNull(supervisor)
                    try {
                        if (!stopped) engine.stop(id, 0).get(20, TimeUnit.SECONDS)
                        engine.remove(id).get(10, TimeUnit.SECONDS)
                    } catch (cleanupFailure: Throwable) {
                        testFailure?.addSuppressed(cleanupFailure) ?: throw cleanupFailure
                    }
                }
            } finally {
                context.unbindService(connection)
            }
        }
    }
}
