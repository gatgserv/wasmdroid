package dev.wasmdroid.app

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.os.IBinder
import android.os.SystemClock
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import dev.wasmdroid.app.engine.EngineService
import dev.wasmdroid.app.engine.ServiceSupervisor
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.io.IOException
import java.net.HttpURLConnection
import java.net.InetSocketAddress
import java.net.Socket
import java.net.URL
import java.util.UUID
import java.util.concurrent.CompletableFuture
import java.util.concurrent.TimeUnit

/** Live registry Python, two isolated Android workers, and their managed host HTTP listeners. */
@RunWith(AndroidJUnit4::class)
class SupervisedHttpAcceptanceTest {
    private data class Running(val id: String, val marker: String, val generation: String, val port: Int)

    @Test
    fun twoPrivateHttpServicesShareGuestPortAndStopIndependently() {
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
        val created = mutableListOf<String>()
        val stopped = mutableSetOf<String>()
        var supervisor: ServiceSupervisor? = null
        var testFailure: Throwable? = null
        try {
            val engine = ready.get(30, TimeUnit.SECONDS)
            supervisor = engine
            val reference = "wasmer.io/python/python@3.13.20"
            engine.pull(reference).get(300, TimeUnit.SECONDS)

            fun start(): Running {
                val marker = UUID.randomUUID().toString()
                val id = "test-http-$marker"
                val ports = JSONArray().put(JSONObject()
                    .put("instance_port", 8080).put("host_port", 0).put("protocol", "tcp"))
                val spec = JSONObject()
                    .put("artifact_ref", reference)
                    .put("command", "python")
                    .put("args", JSONArray().put("-u").put("-c").put(SERVER).put(marker))
                    .put("ports", ports)
                    .put("network", "loopback")
                engine.create(id, spec.toString()).get(20, TimeUnit.SECONDS)
                created += id
                val receipt = JSONObject(engine.start(id).get(180, TimeUnit.SECONDS))
                val binding = receipt.getJSONArray("ports").getJSONObject(0)
                val port = binding.getInt("host_port")
                assertEquals(8080, binding.getInt("instance_port"))
                assertTrue(port in 1..65535)
                return Running(id, marker, receipt.getString("generation"), port)
            }

            fun stop(worker: Running) {
                val receipt = JSONObject(engine.stop(worker.id).get(30, TimeUnit.SECONDS))
                assertEquals(worker.generation, receipt.getString("generation"))
                val instance = receipt.getJSONObject("instance")
                assertEquals(worker.id, instance.getString("id"))
                assertTrue(instance.getString("state") in setOf("exited", "dead"))
                stopped += worker.id
                assertThrows(IOException::class.java) {
                    Socket().use { it.connect(InetSocketAddress("127.0.0.1", worker.port), 1_000) }
                }
            }

            val first = start()
            val second = start()
            assertNotEquals(first.generation, second.generation)
            assertNotEquals(first.port, second.port)
            assertEquals(first.marker, awaitHttp(first.port))
            assertEquals(second.marker, awaitHttp(second.port))

            stop(first)
            assertEquals(second.marker, http(second.port))
            val stillRunning = JSONObject(engine.terminalSession(second.id).get(10, TimeUnit.SECONDS))
            assertEquals(second.generation, stillRunning.getString("generation"))
            stop(second)
        } catch (failure: Throwable) {
            testFailure = failure
            throw failure
        } finally {
            try {
                supervisor?.let { engine ->
                    val failures = created.asReversed().mapNotNull { id ->
                        runCatching {
                            if (id !in stopped) engine.stop(id, 0).get(20, TimeUnit.SECONDS)
                            engine.remove(id).get(10, TimeUnit.SECONDS)
                        }.exceptionOrNull()
                    }
                    if (failures.isNotEmpty()) {
                        val failure = AssertionError("Could not remove test-owned HTTP workers")
                        failures.forEach(failure::addSuppressed)
                        testFailure?.addSuppressed(failure) ?: throw failure
                    }
                }
            } finally {
                context.unbindService(connection)
            }
        }
    }

    private fun awaitHttp(port: Int): String {
        val deadline = SystemClock.elapsedRealtime() + 30_000
        while (true) {
            try {
                return http(port)
            } catch (failure: IOException) {
                if (SystemClock.elapsedRealtime() >= deadline) throw failure
                Thread.sleep(100)
            }
        }
    }

    private fun http(port: Int): String {
        val connection = URL("http://127.0.0.1:$port/").openConnection() as HttpURLConnection
        connection.connectTimeout = 3_000
        connection.readTimeout = 3_000
        connection.instanceFollowRedirects = false
        connection.setRequestProperty("Connection", "close")
        return try {
            assertEquals(200, connection.responseCode)
            connection.inputStream.bufferedReader(Charsets.UTF_8).use { it.readText() }
        } finally {
            connection.disconnect()
        }
    }

    private companion object {
        val SERVER = """
            import socket, sys

            body = sys.argv[1].encode("ascii")
            response = (
                b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: text/plain\r\n"
                + b"Content-Length: " + str(len(body)).encode("ascii") + b"\r\n\r\n" + body
            )
            with socket.socket() as listener:
                listener.bind(("127.0.0.1", 8080))
                listener.listen(8)
                while True:
                    peer, _ = listener.accept()
                    with peer:
                        request = b""
                        while b"\r\n\r\n" not in request and len(request) < 4096:
                            chunk = peer.recv(4096)
                            if not chunk:
                                break
                            request += chunk
                        if b"\r\n\r\n" in request:
                            peer.sendall(response)
        """.trimIndent()
    }
}
