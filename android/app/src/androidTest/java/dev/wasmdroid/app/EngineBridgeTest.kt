package dev.wasmdroid.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import dev.wasmdroid.engine.engineInfo
import dev.wasmdroid.engine.runWasiProbe
import dev.wasmdroid.engine.validateCatalog
import dev.wasmdroid.engine.BridgeException
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import org.junit.Test
import org.junit.runner.RunWith

/** Device integration: loads the packaged .so and calls generated UniFFI bindings. */
@RunWith(AndroidJUnit4::class)
class EngineBridgeTest {
    @Test fun packagedNativeLibraryAnswers() {
        assertEquals(1u, engineInfo().apiVersion)
        assertEquals("local", engineInfo().endpointId)
        assertEquals(BuildConfig.RUNTIME_ENABLED, engineInfo().runtimeProbeAvailable)
        assertEquals(0u, validateCatalog("""{"schema_version":1,"packages":[]}"""))
    }

    @Test fun invalidCatalogCrossesErrorBoundary() {
        assertThrows(BridgeException.Operation::class.java) { validateCatalog("""{"schema_version":99,"packages":[]}""") }
    }

    @Test fun builtInWasiModuleExecutesWhenRuntimeIsIncluded() {
        assumeTrue("Runtime not included in this build", engineInfo().runtimeProbeAvailable)
        assertEquals("Hello from WasmDroid!\n", runWasiProbe())
    }
}
