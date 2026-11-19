package dev.wasmdroid.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import dev.wasmdroid.app.engine.PackageProgress
import dev.wasmdroid.app.ui.packageTasks
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

/** Presentation/host-progress contracts. Does not execute HTTP or Compose. */
@RunWith(AndroidJUnit4::class)
class PackagePresentationTest{
    private fun row(key:String,generation:String,status:String,bytes:Int=0)=JSONObject()
        .put("key",key).put("generation",generation).put("kind","image").put("label","example").put("status",status)
        .put("progress",JSONObject().put("reference","wasmer.io/example/item@1.0.0").put("phase","downloading").put("bytes_downloaded",bytes).put("total_bytes",100))
    private fun frame(active:List<JSONObject>,recent:List<JSONObject>):String=JSONObject()
        .put("active",JSONObject().apply{active.forEach{put(it.getString("key"),it)}})
        .put("recent",JSONArray(recent)).toString()

    @Test fun acceptedCancelStaysLiveUntilOwnerPublishesActualCompletion(){
        val running=row("pull:example","first","cancelled",25)
        val pending=packageTasks(frame(listOf(running),emptyList())).single()
        assertFalse(pending.terminal);assertFalse(pending.cancellable);assertFalse(pending.retryable)
        val completed=packageTasks(frame(emptyList(),listOf(running))).single()
        assertTrue(completed.terminal);assertTrue(completed.retryable)
    }
    @Test fun newerGenerationMasksOldOutcomeWithoutBorrowingItsProgress(){
        val old=row("pull:example","old","failed",90)
        val current=row("pull:example","new","active",5)
        val other=row("pull:other","other","active",30)
        val rows=packageTasks(frame(listOf(current,other),listOf(old)))
        assertEquals(2,rows.size)
        val latest=rows.first{it.key=="pull:example"}
        assertEquals("new",latest.generation);assertEquals(5L,latest.bytes);assertTrue(latest.cancellable)
        assertEquals(30L,rows.first{it.key=="pull:other"}.bytes)
        assertNotEquals(latest.identity,packageTasks(frame(emptyList(),listOf(old))).single().identity)
    }
    @Test fun imageCommitOrInstalledPhaseDoesNotClaimCatalogApplicationReady(){
        val committed=row("install","recipe","committed",100)
        committed.getJSONObject("progress").put("phase","installed")
        val pending=packageTasks(frame(listOf(committed),emptyList())).single()
        assertFalse(pending.terminal);assertFalse(pending.cancellable);assertFalse(pending.retryable)
        val failed=committed.put("status","failed").put("image_committed",true).put("error_kind","space")
        val outcome=packageTasks(frame(emptyList(),listOf(failed))).single()
        assertEquals("failed",outcome.status);assertTrue(outcome.imageCommitted);assertTrue(outcome.retryable)
    }
    @Test fun verifiedRegistryDownloadUsesTheRequestedReleaseEvenWhenProgressNamesADependency(){
        val release = "wasmer.io/example/root@2.0.0"
        val result = row("pull:$release", "verified", "installed", 100)
        val task = packageTasks(frame(emptyList(), listOf(result))).single()
        assertEquals("wasmer.io/example/item@1.0.0", task.reference)
        assertEquals(release, task.downloadedRegistryReference)
        assertNull(task.copy(finished = false).downloadedRegistryReference)
        assertNull(task.copy(status = "failed").downloadedRegistryReference)
        assertNull(task.copy(status = "cancelled").downloadedRegistryReference)
        assertNull(task.copy(kind = "application").downloadedRegistryReference)
    }
    @Test fun progressFiltersTemporaryUrlsPrivateFieldsAndInvalidCounters(){
        val valid=JSONObject().put("reference","wasmer.io/example/item@1.0.0").put("phase","downloading").put("bytes_downloaded",4).put("total_bytes",10)
            .put("download_url","https://example.invalid/synthetic-private-marker").put("env",JSONObject().put("SECRET","synthetic-env-marker"))
        val event=PackageProgress.parse(valid.toString())!!
        assertFalse(event.json().toString().contains("synthetic-"))
        assertNull(PackageProgress.parse(JSONObject(valid.toString()).put("reference","https://example.invalid/private").toString()))
        assertNull(PackageProgress.parse(JSONObject(valid.toString()).put("bytes_downloaded",11).toString()))
        assertNull(PackageProgress.parse(JSONObject(valid.toString()).put("bytes_downloaded",-1).toString()))
        assertNull(PackageProgress.parse(JSONObject(valid.toString()).put("phase","private-unknown").toString()))
    }
}
