package dev.wasmdroid.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import dev.wasmdroid.engine.EngineStorage
import dev.wasmdroid.engine.InstallControl
import dev.wasmdroid.engine.InstallStatus
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.security.MessageDigest
import java.util.UUID

/** Native format/transaction boundary only. This does not execute a guest or
 * certify common-worker stop, SAF, UI, background or whole-process limits. */
@RunWith(AndroidJUnit4::class)
class LocalPackageImportTest {
    // (module (func (export "_start"))). No native serialized compiled code.
    private val wasm = byteArrayOf(0,97,115,109,1,0,0,0,1,4,1,96,0,0,3,2,1,0,
        7,10,1,6,95,115,116,97,114,116,0,0,10,4,1,2,0,11)

    @Test fun streamsActualWasmReviewsDigestThenCommitsWithoutStartingAnything() = isolated { engine ->
        val control = InstallControl()
        engine.beginPackageImport(wasm.size.toULong(), (wasm.size*2).toULong(), null, control).use { transfer ->
            assertEquals(11uL, transfer.write(wasm.copyOfRange(0,11)))
            transfer.write(wasm.copyOfRange(11,wasm.size))
            val preview = JSONObject(transfer.preview())
            val artifact = preview.getJSONObject("artifact")
            val hash = MessageDigest.getInstance("SHA-256").digest(wasm).joinToString("") { "%02x".format(it.toInt() and 255) }
            assertEquals("sha256:$hash", artifact.getString("digest"))
            assertEquals("local.wasmdroid/sha256/$hash", artifact.getString("reference"))
            assertEquals("wasm", artifact.getString("kind"))
            assertEquals("main", artifact.getJSONArray("commands").getString(0))
            assertEquals(0, JSONArray(engine.listImages()).length())
            rejects { transfer.write(byteArrayOf(0)) }
            rejects { transfer.commit("not-reviewed") }
            val installed = JSONObject(transfer.commit(preview.getString("token")))
            assertEquals(artifact.getString("digest"), installed.getString("digest"))
            assertEquals(InstallStatus.COMMITTED, control.status())
            assertFalse(control.cancel())
            assertEquals(1, JSONArray(engine.listImages()).length())
            assertEquals(0, JSONArray(engine.listInstances("local")).length())
            assertEquals(0, JSONArray(engine.listVolumes()).length())
        }
    }

    @Test fun rejectsNonWasmTruncationAndAcceptedCancellationBeforePublication() = isolated { engine ->
        for (bytes in listOf(byteArrayOf(127,69,76,70), wasm.copyOf(wasm.size-1), byteArrayOf(0,97,115,109,13,0,1,0))) {
            engine.beginPackageImport(bytes.size.toULong(), 4096uL, null, InstallControl()).use { transfer ->
                transfer.write(bytes)
                rejects { transfer.preview() }
                transfer.abort()
            }
        }
        val control = InstallControl()
        engine.beginPackageImport(wasm.size.toULong(), 4096uL, null, control).use { transfer ->
            transfer.write(wasm)
            val token = JSONObject(transfer.preview()).getString("token")
            assertTrue(control.cancel())
            rejects { transfer.commit(token) }
            transfer.abort()
            assertEquals(InstallStatus.CANCELLED, control.status())
        }
        assertEquals(0, JSONArray(engine.listImages()).length())
    }

    private fun rejects(block: () -> Unit) {
        var failed = false
        try { block() } catch (expected: Exception) { failed = true }
        assertTrue("Native boundary must reject this input/state", failed)
    }
    private fun isolated(block: (EngineStorage) -> Unit) {
        val base = InstrumentationRegistry.getInstrumentation().targetContext.cacheDir.canonicalFile
        val root = File(base, "test-local-package-${UUID.randomUUID()}").canonicalFile
        check(root.parentFile == base && root.name.startsWith("test-local-package-"))
        root.mkdirs()
        try { EngineStorage.open(root.absolutePath).use(block) }
        finally {
            check(root.canonicalFile.parentFile == base && root.name.startsWith("test-local-package-"))
            root.deleteRecursively()
        }
    }
}
