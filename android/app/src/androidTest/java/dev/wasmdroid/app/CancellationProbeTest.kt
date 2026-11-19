package dev.wasmdroid.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import dev.wasmdroid.engine.engineInfo
import dev.wasmdroid.engine.runCancellationProbe
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class CancellationProbeTest {
    @Test fun interruptsCpuBoundWasixWithoutStoppingAnotherInstance() {
        assertTrue(engineInfo().runtimeProbeAvailable)
        val json = runCancellationProbe()
        val result = JSONObject(json)
        assertTrue(result.getBoolean("first_host_interrupt_confirmed"))
        assertTrue(result.getBoolean("second_host_interrupt_confirmed"))
        assertEquals(0, result.getInt("first_active_tasks_after_stop"))
        assertEquals(0, result.getInt("second_active_tasks_after_stop"))
        assertTrue(result.getBoolean("second_was_running_after_first_stop"))
        InstrumentationRegistry.getInstrumentation().sendStatus(2, android.os.Bundle().apply {
            putString("stream", "WASMDROID_CANCELLATION_JSON=$json\n")
        })
    }
}
