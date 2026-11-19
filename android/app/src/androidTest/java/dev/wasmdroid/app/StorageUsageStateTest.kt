package dev.wasmdroid.app

import dev.wasmdroid.app.ui.*
import kotlinx.coroutines.*
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

/** Pure typed backend cases can run on a host without executing JSONObject.
 * Deferred replies intentionally survive cancelled UI observation. */
class StorageUsageStateTest {
    private class Backend:StorageUsageBackend {
        override var current=true
        val calls=mutableListOf<CompletableDeferred<StorageUsageSnapshot>>()
        override suspend fun snapshot():StorageUsageSnapshot {
            val answer=CompletableDeferred<StorageUsageSnapshot>();calls+=answer
            return withContext(NonCancellable){answer.await()}
        }
    }
    private class Fixture:AutoCloseable {
        val scope=CoroutineScope(SupervisorJob()+Dispatchers.Unconfined)
        val backends=mutableListOf(Backend())
        var backend=backends.first()
        var captureFailure:Exception?=null
        val controller=StorageUsageController(scope,{captureFailure?.let{throw it};backend})
        init{controller.availability(true)}
        fun replace(){backend.current=false;controller.availability(false);backend=Backend().also{backends+=it};controller.availability(true)}
        override fun close(){controller.close();backends.flatMap{it.calls}.forEach{it.complete(frame())};scope.cancel()}
    }
    @Test fun refreshCoalescesWhileLoadingAndOnlyActualReplySuppliesCounts()=Fixture().use{f->
        f.controller.refresh();f.controller.refresh()
        assertEquals(1,f.backend.calls.size)
        assertEquals(StorageUsagePhase.Loading,f.controller.state.value.phase)
        assertNull(f.controller.state.value.snapshot)
        f.backend.calls.single().complete(frame(42uL))
        assertEquals(StorageUsagePhase.Ready,f.controller.state.value.phase)
        assertEquals(42uL,f.controller.state.value.snapshot!!.observedTotalBytes)
    }
    @Test fun ownerReconnectDoesNotFetchOrAcceptThePreviousOwnersLateReply()=Fixture().use{f->
        f.controller.refresh();val old=f.backend.calls.single()
        f.replace()
        assertEquals(StorageUsagePhase.Unavailable,f.controller.state.value.phase)
        assertTrue(f.backend.calls.isEmpty())
        f.controller.refresh();f.backend.calls.single().complete(frame(99uL))
        old.complete(frame(1uL))
        assertEquals(99uL,f.controller.state.value.snapshot!!.observedTotalBytes)
    }
    @Test fun staleFailureCannotEraseANewerOwnersSuccessfulObservation()=Fixture().use{f->
        f.controller.refresh();val old=f.backend.calls.single()
        f.replace();f.controller.refresh();f.backend.calls.single().complete(frame(77uL))
        old.completeExceptionally(Exception("private old failure"))
        assertEquals(StorageUsagePhase.Ready,f.controller.state.value.phase)
        assertEquals(77uL,f.controller.state.value.snapshot!!.observedTotalBytes)
    }
    @Test fun disconnectImmediatelyWithholdsAcceptedValuesAndReconnectRequiresRefresh()=Fixture().use{f->
        f.controller.refresh();f.backend.calls.single().complete(frame(7uL))
        f.controller.availability(false)
        assertNull(f.controller.state.value.snapshot)
        f.controller.availability(true)
        assertEquals(StorageUsagePhase.Unavailable,f.controller.state.value.phase)
        assertEquals(1,f.backend.calls.size)
        f.controller.refresh();assertEquals(2,f.backend.calls.size)
    }
    @Test fun currentBackendFailureWithholdsPriorValuesAndRetryIsExplicit()=Fixture().use{f->
        f.controller.refresh();f.backend.calls.single().complete(frame(7uL))
        f.controller.refresh();assertNull(f.controller.state.value.snapshot)
        f.backend.calls.last().completeExceptionally(Exception("private error not retained"))
        assertEquals(StorageUsagePhase.Failed,f.controller.state.value.phase)
        assertNull(f.controller.state.value.snapshot)
        assertFalse(f.controller.state.value.toString().contains("private error"))
        assertEquals(2,f.backend.calls.size)
        f.controller.refresh();assertEquals(3,f.backend.calls.size)
    }
    @Test fun changedOwnerBeforeOrAfterReadCannotPublishEvenWithoutCallbackDelivery()=Fixture().use{f->
        f.backend.current=false;f.controller.refresh()
        assertTrue(f.backend.calls.isEmpty())
        assertEquals(StorageUsagePhase.Unavailable,f.controller.state.value.phase)
        f.backend.current=true;f.controller.refresh()
        f.backend.current=false;f.backend.calls.single().complete(frame())
        assertEquals(StorageUsagePhase.Unavailable,f.controller.state.value.phase)
        assertNull(f.controller.state.value.snapshot)
    }
    @Test fun closeDiscardsLateReplyButDoesNotCompleteThePrivatelyRetainedScan()=Fixture().use{f->
        f.controller.refresh();val actual=f.backend.calls.single()
        f.controller.close()
        assertTrue(actual.isActive)
        actual.complete(frame())
        f.controller.refresh();f.controller.availability(true)
        assertNull(f.controller.state.value.snapshot)
        assertEquals(StorageUsagePhase.Unavailable,f.controller.state.value.phase)
        assertEquals(1,f.backend.calls.size)
    }
    @Test fun pendingCaptureFromOldEpochCannotDispatchAfterReconnect() {
        val scope=CoroutineScope(SupervisorJob()+Dispatchers.Unconfined)
        val first=CompletableDeferred<StorageUsageBackend>()
        val old=Backend();val replacement=Backend();var captures=0
        val controller=StorageUsageController(scope,{
            captures++
            if(captures==1)withContext(NonCancellable){first.await()}else replacement
        })
        try{
            controller.availability(true);controller.refresh()
            controller.availability(false);controller.availability(true);controller.refresh()
            replacement.calls.single().complete(frame(9uL))
            first.complete(old)
            assertTrue(old.calls.isEmpty())
            assertEquals(9uL,controller.state.value.snapshot!!.observedTotalBytes)
        }finally{controller.close();first.complete(old);scope.cancel()}
    }
    @Test fun captureFailureCannotExposeAFalseEmptyStorageObservation()=Fixture().use{f->
        f.captureFailure=Exception("private capture failure")
        f.controller.refresh()
        assertEquals(StorageUsagePhase.Failed,f.controller.state.value.phase)
        assertNull(f.controller.state.value.snapshot)
        assertTrue(f.backend.calls.isEmpty())
    }
    @Test fun formatterPreservesUnsignedRangeAndUsesSafeBinaryRounding() {
        assertEquals("0 B",formatStorageBytes(0uL))
        assertEquals("1023 B",formatStorageBytes(1023uL))
        assertEquals("1.0 KiB",formatStorageBytes(1024uL))
        assertEquals("1.5 KiB",formatStorageBytes(1536uL))
        assertEquals("1.6 KiB",formatStorageBytes(1588uL))
        assertEquals("1.0 MiB",formatStorageBytes(1024uL*1024uL))
        assertEquals("8.0 EiB",formatStorageBytes(Long.MAX_VALUE.toULong()+1uL))
        assertEquals("16.0 EiB",formatStorageBytes(ULong.MAX_VALUE))
    }
    companion object {
        private fun frame(bytes:ULong=3uL)=StorageUsageSnapshot(StorageCategory.entries.map{category->
            StorageCategoryUsage(category,StorageCountStatus.Complete,if(category==StorageCategory.Artifacts)bytes else 0uL,
                if(category==StorageCategory.Artifacts)1 else 0,if(category==StorageCategory.Artifacts)1 else 0,emptySet())
        },true,bytes,0uL,1,emptySet())
    }
}

/** Android JSONObject definitions. Compile on integration; never include this
 * class in the pure host runner or claim Android parsing from stub execution. */
class StorageUsageCodecTest {
    private fun frame():String="""{"schema":1,"metric":"logical_regular_file_bytes","consistency":"best_effort_non_atomic","categories":[{"category":"artifacts","status":"complete","logical_regular_file_bytes":3,"files_counted":1,"entries_visited":1,"issues":[]},{"category":"compiled_cache","status":"complete","logical_regular_file_bytes":0,"files_counted":0,"entries_visited":0,"issues":[]},{"category":"volumes","status":"complete","logical_regular_file_bytes":0,"files_counted":0,"entries_visited":0,"issues":[]},{"category":"logs","status":"complete","logical_regular_file_bytes":0,"files_counted":0,"entries_visited":0,"issues":[]}],"complete":true,"observed_total_bytes":3,"entries_visited":1,"issues":[],"available_bytes":0,"available_status":"available"}"""
    private fun rejects(raw:String){try{StorageUsageCodec.parse(raw);fail("Invalid storage frame must be rejected")}catch(_:Exception){}}
    @Test fun validZeroAvailableAndLogicalCountsRemainDistinct() {
        val parsed=StorageUsageCodec.parse(frame())
        assertEquals(3uL,parsed.observedTotalBytes);assertEquals(0uL,parsed.availableBytes)
        assertTrue(parsed.complete);assertEquals(4,parsed.categories.size)
    }
    @Test fun partialCountsStayPartialAndUnknownCategoryNeverBecomesZero() {
        val partial=JSONObject(frame());val row=partial.getJSONArray("categories").getJSONObject(0)
        row.put("status","partial").put("issues",JSONArray().put("entry_limit"));partial.put("complete",false)
        val observed=StorageUsageCodec.parse(partial.toString())
        assertFalse(observed.complete);assertEquals(3uL,observed.observedTotalBytes)
        row.put("status","unavailable").put("logical_regular_file_bytes",JSONObject.NULL)
            .put("files_counted",0).put("entries_visited",0).put("issues",JSONArray().put("permission_denied"))
        partial.put("entries_visited",0).put("observed_total_bytes",JSONObject.NULL).put("available_bytes",64)
        val unknown=StorageUsageCodec.parse(partial.toString())
        assertNull(unknown.observedTotalBytes);assertNull(unknown.categories.first().bytes)
        assertEquals(64uL,unknown.availableBytes)
    }
    @Test fun coercedOrLossyIntegersAndBooleansAreRejected() {
        for(value in listOf("-1","3.0","3e0","true","\"3\"","18446744073709551615"))
            rejects(frame().replace("\"logical_regular_file_bytes\":3","\"logical_regular_file_bytes\":$value"))
        rejects(frame().replace("\"complete\":true","\"complete\":\"true\""))
        rejects(frame().replace("\"schema\":1","\"schema\":\"1\""))
    }
    @Test fun wrongSchemaMetricCategoriesOrUnknownFieldsAreRejected() {
        rejects(frame().replace("\"schema\":1","\"schema\":2"))
        rejects(frame().replace("logical_regular_file_bytes\",\"consistency","resident_memory\",\"consistency"))
        rejects(frame().replace("best_effort_non_atomic","atomic"))
        rejects(frame().replace("\"category\":\"logs\"","\"category\":\"volumes\""))
        rejects(frame().replace("\"category\":\"logs\"","\"category\":\"unknown\""))
        rejects(JSONObject(frame()).put("private_path","not accepted").toString())
    }
    @Test fun inconsistentSumsStatusCompletenessOrAvailableFieldsAreRejected() {
        rejects(frame().replace("\"observed_total_bytes\":3","\"observed_total_bytes\":4"))
        rejects(frame().replace("\"complete\":true","\"complete\":false"))
        rejects(frame().replace("\"available_status\":\"available\"","\"available_status\":\"unavailable\""))
        val wrong=JSONObject(frame());wrong.getJSONArray("categories").getJSONObject(0).put("files_counted",2)
        rejects(wrong.toString())
        wrong.getJSONArray("categories").getJSONObject(0).put("files_counted",0)
        rejects(wrong.toString())
    }
    @Test fun singleRootRejectsTrailingObjectsPrimitivesAndLiteralNul() {
        for(suffix in listOf(" {}"," []"," true"," 42","\u0000"))rejects(frame()+suffix)
        assertEquals(3uL,StorageUsageCodec.parse(" \n"+frame()+" \n").observedTotalBytes)
    }
    @Test fun oversizedOrDeepInputIsRefusedBeforePlatformRecursiveParsing() {
        rejects(" ".repeat(16*1024)+frame())
        rejects("[".repeat(9)+frame()+"]".repeat(9))
    }
}
