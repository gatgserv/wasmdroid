package dev.wasmdroid.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import dev.wasmdroid.app.engine.PackageProgress
import dev.wasmdroid.app.ui.AppState
import dev.wasmdroid.app.ui.LocalPackageView
import dev.wasmdroid.app.ui.PackageTask
import dev.wasmdroid.app.ui.packageTasks
import dev.wasmdroid.app.ui.packageWorkActive
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class LocalPackagePresentationTest {
    private fun frame():JSONObject {
        val hash="a".repeat(64)
        val artifact=JSONObject().put("reference","local.wasmdroid/sha256/$hash").put("digest","sha256:$hash")
            .put("driver","wasmer-local").put("kind","webc").put("size_bytes",12)
            .put("commands",JSONArray().put("main").put("other"))
        val preview=JSONObject().put("schema_version",1).put("token","sha256:${"b".repeat(64)}")
            .put("artifact",artifact).put("pins",JSONArray()).put("missing",JSONArray())
        return JSONObject().put("key","package:local:fixture").put("generation","current").put("name","selected.webc")
            .put("status","awaiting_review").put("busy",false).put("preview",preview)
    }
    @Test fun sourceNamesNeverChooseArtifactIdentityOrCommandsAndMissingDepsHaveExplicitSearchTargets(){
        val raw=frame()
        raw.put("name","claimed-publisher.webc")
        raw.getJSONObject("preview").getJSONArray("missing").put(JSONObject().put("type","named")
            .put("reference","wasmer.io/example/runtime").put("version_requirement","^1.0"))
        val view=LocalPackageView.parse(raw.toString())
        val preview=view.preview!!
        assertEquals("example/runtime",preview.missing.single().query)
        val spec=JSONObject(preview.spec("other"))
        assertEquals("other",spec.getString("command"));assertEquals(preview.reference,spec.getString("artifact_ref"))
        assertFalse(spec.toString().contains("claimed-publisher"));assertFalse(spec.has("start"))
        assertEquals(0,spec.getJSONArray("mounts").length());assertEquals(0,spec.getJSONArray("ports").length())
        rejects{preview.spec("not-a-command")}
        raw.getJSONObject("preview").getJSONObject("artifact").put("reference","wasmer.io/claimed/publisher")
        rejects{LocalPackageView.parse(raw.toString())}
    }
    @Test fun reviewingImportsDoNotBlockDependencyInstallsAndDoNotBecomeTerminalReceipts(){
        val row=JSONObject().put("key","package:local:fixture").put("generation","current").put("kind","local_import")
            .put("status","awaiting_review")
        val snapshot=JSONObject().put("active",JSONObject().put("package:local:fixture",row)).put("recent",JSONArray())
        val task=packageTasks(snapshot.toString()).single()
        assertFalse(task.terminal);assertTrue(task.cancellable)
        assertFalse(AppState(packages=listOf(task)).packageWorkActive())
        assertTrue(AppState(packages=listOf(task.copy(status="active"))).packageWorkActive())
        assertFalse(task.copy(status="cancelled",finished=true).retryable)
        // Unknown provider size remains unknown; no fake percent or source URI.
        val progress=JSONObject().put("reference","").put("phase","reading_local").put("bytes_downloaded",1024).put("total_bytes",0)
        assertEquals(1024L,PackageProgress.parse(progress.toString())!!.bytes)
        progress.put("reference","content://private/document")
        assertNull(PackageProgress.parse(progress.toString()))
        assertFalse(PackageTask("key","generation","local_import","","installed",finished=false).terminal)
    }
    private fun rejects(block:()->Unit){var rejected=false;try{block()}catch(expected:Exception){rejected=true};assertTrue(rejected)}
}
