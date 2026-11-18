package dev.wasmdroid.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import dev.wasmdroid.engine.EngineStorage
import dev.wasmdroid.engine.InstallControl
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.util.UUID

/** Actual importer/native metadata definitions; no guest or UI acceptance. */
@RunWith(AndroidJUnit4::class)
class LocalStackImportTest {
    private val wasm=byteArrayOf(0,97,115,109,1,0,0,0,1,4,1,96,0,0,3,2,1,0,
        7,10,1,6,95,115,116,97,114,116,0,0,10,4,1,2,0,11)
    @Test fun verifiedRawImportCreatesAndUpdatesManagedMetadataWithoutGuestStart(){
        val base=InstrumentationRegistry.getInstrumentation().targetContext.cacheDir.canonicalFile
        val root=File(base,"test-local-stack-${UUID.randomUUID()}").canonicalFile
        check(root.parentFile==base&&root.name.startsWith("test-local-stack-"));root.mkdirs()
        try{
            EngineStorage.open(root.absolutePath).use{db->
                val reference=db.beginPackageImport(wasm.size.toULong(),4096uL,null,InstallControl()).use{transfer->
                    transfer.write(wasm)
                    val preview=JSONObject(transfer.preview())
                    JSONObject(transfer.commit(preview.getString("token"))).getString("reference")
                }
                val document=JSONObject().put("services",JSONObject().put("app",JSONObject().put("image",reference)
                    .put("volumes",JSONArray().put("data:/data")))).put("volumes",JSONObject().put("data",JSONObject())).toString()
                val preview=db.previewStack("imported",document)
                assertTrue(JSONObject(preview).getJSONObject("services").getJSONObject("app").has("local_metadata"))
                val created=JSONObject(db.createStackFromPreview("imported",preview))
                assertEquals(reference,JSONObject(db.stackPlan("imported")).getJSONObject("plan").getJSONObject("services").getJSONObject("app").getJSONObject("spec").getString("artifact_ref"))
                val member=created.getJSONArray("instances").getJSONObject(0)
                assertEquals("wasm",member.getString("kind"));assertEquals("created",member.getString("state"))
                assertEquals(reference,member.getJSONObject("spec").getString("artifact_ref"))
                val editable=JSONObject(db.stackEditSource("imported"))
                val next=JSONObject(editable.getString("document"))
                next.getJSONObject("services").getJSONObject("app").put("environment",JSONObject().put("EXPLICIT_VALUE","two words"))
                val update=db.previewStackUpdate("imported",created.getLong("version").toULong(),next.toString())
                val applied=JSONObject(db.updateStackFromPreview("imported",update))
                val changed=applied.getJSONArray("instances").getJSONObject(0)
                assertEquals(member.getString("id"),changed.getString("id"));assertEquals("created",changed.getString("state"))
                assertEquals("two words",changed.getJSONObject("spec").getJSONObject("env").getString("EXPLICIT_VALUE"))
                assertFalse(db.eventsAfter(0uL,100u).contains("two words"))
                db.removeStack("imported",applied.getLong("version").toULong())
                assertEquals(1,JSONArray(db.listVolumes()).length())
            }
        }finally{
            check(root.canonicalFile.parentFile==base&&root.name.startsWith("test-local-stack-"));root.deleteRecursively()
        }
    }
}
