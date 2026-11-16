package dev.wasmdroid.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import dev.wasmdroid.engine.ProbeObserver
import dev.wasmdroid.engine.engineInfo
import dev.wasmdroid.engine.runRegistryBenchmark
import java.io.File
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

/** Explicit measurements; no speed threshold and no mixing download/startup time. */
@RunWith(AndroidJUnit4::class)
class RuntimeBenchmarkTest {
    private fun evidence(message: String) {
        InstrumentationRegistry.getInstrumentation().sendStatus(2, android.os.Bundle().apply {
            putString("stream", "$message\n")
        })
    }
    @Test fun measureVerifiedPythonColdAndSameProcessWarmRuns() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        assertTrue(engineInfo().runtimeProbeAvailable)
        val observer = object : ProbeObserver {
            override fun onPhase(phase: String) { evidence(phase) }
        }
        val json = runRegistryBenchmark(File(context.filesDir, "registry-probe").absolutePath, observer)
        val result = JSONObject(json)
        assertEquals(1, result.getInt("schema_version"))
        assertEquals(engineInfo().runtimeBackend, result.getString("backend"))
        assertEquals(3, result.getJSONArray("warm_start_to_exit_us").length())
        assertEquals(4, result.getJSONArray("outputs").length())
        assertTrue(result.getLong("cold_start_to_exit_us") >= result.getLong("cold_compile_us"))
        evidence("WASMDROID_BENCHMARK_JSON=$json")
    }
}
