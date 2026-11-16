package dev.wasmdroid.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import dev.wasmdroid.engine.ProbeObserver
import dev.wasmdroid.engine.engineInfo
import dev.wasmdroid.engine.startRegistryHttpProbe
import java.io.File
import java.net.HttpURLConnection
import java.net.InetSocketAddress
import java.net.Socket
import java.net.URL
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

/** Live registry server; separate from the hermetic bridge suite. */
@RunWith(AndroidJUnit4::class)
class HttpProbeTest {
    private fun evidence(message: String) {
        InstrumentationRegistry.getInstrumentation().sendStatus(2, android.os.Bundle().apply {
            putString("stream", "$message\n")
        })
    }

    @Test fun registryServerRespondsThroughManagedLoopbackAndStops() {
        assertTrue("Build requires Wasmer", engineInfo().runtimeProbeAvailable)
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val networkPolicy = android.security.NetworkSecurityPolicy.getInstance()
        assertTrue(networkPolicy.isCleartextTrafficPermitted("127.0.0.1"))
        assertFalse(networkPolicy.isCleartextTrafficPermitted("example.com"))
        assertFalse(networkPolicy.isCleartextTrafficPermitted("192.0.2.1"))
        val observer = object : ProbeObserver {
            override fun onPhase(phase: String) { evidence("WasmDroid HTTP phase: $phase") }
        }
        val server = startRegistryHttpProbe(File(context.filesDir, "registry-probe").absolutePath, observer)
        val info = server.info()
        val url = URL(info.url)
        var requestFailure: Throwable? = null
        try {
            assertEquals("wasmer.io/wasmer/static-web-server@2.44.0", info.reference)
            assertEquals("sha256:1e84109da307837d8160d23a77c36403ab44f7ec0c592b1cfe4a5bd79c968e08", info.digest)
            assertEquals("127.0.0.1", url.host)
            assertTrue(url.port > 0)
            repeat(2) {
                val connection = url.openConnection() as HttpURLConnection
                connection.connectTimeout = 30_000
                connection.readTimeout = 30_000
                connection.setRequestProperty("Connection", "close")
                try {
                    evidence("HTTP response: status=${connection.responseCode}")
                    assertEquals(200, connection.responseCode)
                    val body = connection.inputStream.bufferedReader().use { it.readText() }
                    assertTrue(body, body.contains("WASMDROID_HTTP_OK"))
                    assertTrue(body, body.contains("<p>${info.documentToken}</p>"))
                    evidence("Verified HTTP body marker: WASMDROID_HTTP_OK")
                    assertFalse(body.contains("host-only-sentinel"))
                } finally { connection.disconnect() }
            }
        } catch (failure: Throwable) {
            requestFailure = failure
            evidence("HTTP request failed: $failure")
            throw failure
        } finally {
            val result = try { server.stop() } catch (stopFailure: Throwable) {
                if (requestFailure != null) { requestFailure.addSuppressed(stopFailure); throw requestFailure }
                throw stopFailure
            }
            evidence("HTTP stop: exit=${result.exitCode}, forced=${result.forced}, activeTasks=${result.activeExecutionTasks}, elapsed=${result.elapsedMs} ms, stderr=${result.stderr}")
            assertEquals("Guest execution work must really unwind", 0u, result.activeExecutionTasks)
        }
        try {
            Socket().use { it.connect(InetSocketAddress(url.host, url.port), 1_000) }
            fail("Managed listener must be closed after stop")
        } catch (_: java.io.IOException) { /* actual host socket no longer accepts */ }
        server.stop() // idempotent, including output/result retention
        server.close()
    }

    private fun body(url: String): String {
        val connection = URL(url).openConnection() as HttpURLConnection
        connection.connectTimeout = 30_000
        connection.readTimeout = 30_000
        connection.setRequestProperty("Connection", "close")
        return try {
            assertEquals(200, connection.responseCode)
            connection.inputStream.bufferedReader().use { it.readText() }
        } finally { connection.disconnect() }
    }

    @Test fun simultaneousGuestPortsAndFilesStayIsolatedWhenOneStops() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val directory = File(context.filesDir, "registry-probe").absolutePath
        fun observer(name: String) = object : ProbeObserver {
            override fun onPhase(phase: String) { evidence("HTTP isolation $name: $phase") }
        }
        val first = startRegistryHttpProbe(directory, observer("first"))
        try {
            val second = startRegistryHttpProbe(directory, observer("second"))
            try {
                evidence("HTTP isolation: both instances started")
                val one = first.info()
                val two = second.info()
                assertNotEquals(one.url, two.url)
                assertNotEquals(one.documentToken, two.documentToken)
                val firstBody = body(one.url)
                evidence("HTTP isolation: first response received")
                val secondBody = body(two.url)
                evidence("HTTP isolation: second response received")
                assertTrue(firstBody.contains("<p>${one.documentToken}</p>"))
                assertFalse(firstBody.contains("<p>${two.documentToken}</p>"))
                assertTrue(secondBody.contains("<p>${two.documentToken}</p>"))
                assertFalse(secondBody.contains("<p>${one.documentToken}</p>"))
                assertEquals(0u, first.stop().activeExecutionTasks)
                evidence("HTTP isolation: first stopped")
                assertTrue(body(two.url).contains("<p>${two.documentToken}</p>"))
                evidence("Two guest servers used private port 8080 and distinct host ports/document roots; second remained reachable after first stopped")
            } finally {
                evidence("HTTP isolation: stopping second")
                assertEquals(0u, second.stop().activeExecutionTasks)
                evidence("HTTP isolation: closing second")
                second.close()
                evidence("HTTP isolation: second closed")
            }
        } finally {
            evidence("HTTP isolation: stopping/closing first")
            assertEquals(0u, first.stop().activeExecutionTasks)
            first.close()
            evidence("HTTP isolation: first closed")
        }
    }
}
