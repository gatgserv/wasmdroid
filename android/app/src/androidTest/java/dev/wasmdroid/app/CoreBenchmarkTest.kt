package dev.wasmdroid.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import dev.wasmdroid.engine.engineInfo
import dev.wasmdroid.engine.runCoreBenchmark
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class CoreBenchmarkTest {
    @Test fun integerWorkloadReturnsVerifiedChecksumAndRawTimings() {
        assertTrue(engineInfo().runtimeProbeAvailable)
        val json = runCoreBenchmark()
        val result = JSONObject(json)
        assertEquals(engineInfo().runtimeBackend, result.getString("backend"))
        assertEquals(1_000_000, result.getInt("iterations_per_sample"))
        assertEquals(5, result.getJSONArray("execution_us").length())
        InstrumentationRegistry.getInstrumentation().sendStatus(2, android.os.Bundle().apply {
            putString("stream", "WASMDROID_CORE_BENCHMARK_JSON=$json\n")
        })
    }
}
