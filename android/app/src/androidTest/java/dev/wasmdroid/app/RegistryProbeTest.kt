package dev.wasmdroid.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import dev.wasmdroid.engine.engineInfo
import dev.wasmdroid.engine.runRegistryPythonProbe
import dev.wasmdroid.engine.ProbeObserver
import java.io.File
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

/** Explicit network-dependent suite; selected separately from hermetic bridge checks. */
@RunWith(AndroidJUnit4::class)
class RegistryProbeTest {
    @Test fun downloadsVerifiedWebcAndExecutesPythonInGuestFilesystem() {
        assertTrue("Build requires Wasmer", engineInfo().runtimeProbeAvailable)
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        File(context.filesDir, "host-only-sentinel").writeText("test-owned host fixture")
        val observer=object:ProbeObserver { override fun onPhase(phase:String) { println("WasmDroid probe phase: $phase") } }
        val result = runRegistryPythonProbe(File(context.filesDir, "registry-probe").absolutePath,observer)
        assertEquals("wasmer.io/python/python@3.13.20", result.reference)
        assertEquals("sha256:a9fa8202f1bf6a4eca8d31ee5f2b1970c8f45af5c5520cc954d28a133333d8e7", result.digest)
        assertTrue(result.installedImages >= 3u)
        assertEquals(result.stderr, 0, result.exitCode)
        assertTrue(result.stdout, result.stdout.contains("WASMDROID_PYTHON_OK"))
        assertTrue(result.stdout, result.stdout.contains("WASMDROID_NETWORK_DENIED"))
        println("Registry Python probe: ${result.reference}, ${result.digest}, ${result.elapsedMs} ms")
    }
}
