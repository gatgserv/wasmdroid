package dev.wasmdroid.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import dev.wasmdroid.engine.EngineStorage
import dev.wasmdroid.engine.runStorageProbe
import java.io.File
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class StorageProbeTest {
    @Test fun privateVolumesPersistAndEnforceReadOnlyLeasesAndPathIsolation() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val json = runStorageProbe(File(context.filesDir, "storage-probe").absolutePath)
        val result = JSONObject(json)
        for (key in listOf("read_only_enforced", "lease_prevented_remove", "escape_denied", "removed")) {
            assertTrue(key, result.getBoolean(key))
        }
        assertTrue(result.getInt("persisted_bytes") > 0)
        InstrumentationRegistry.getInstrumentation().sendStatus(2, android.os.Bundle().apply {
            putString("stream", "WASMDROID_STORAGE_JSON=$json\n")
        })
    }

    @Test fun generatedStorageObjectCreatesListsAndRemovesOnlyOwnedFixtureVolume() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val directory = File(context.filesDir, "storage-facade-fixture").absolutePath
        val storage = EngineStorage.open(directory)
        try {
            val name = "owned-test-volume"
            val created = JSONObject(storage.createVolume(name))
            assertEquals(name, created.getString("name"))
            assertEquals("local-fs", created.getString("driver"))
            assertEquals(1, JSONArray(storage.listVolumes()).length())
            storage.removeVolume(name)
            assertEquals(0, JSONArray(storage.listVolumes()).length())
            assertTrue(JSONArray(storage.eventsAfter(0uL, 100u)).length() >= 2)
        } finally { storage.close() }
    }
}
