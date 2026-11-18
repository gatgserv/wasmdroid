package dev.wasmdroid.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import dev.wasmdroid.app.ui.*
import kotlinx.coroutines.*
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

/** Controller definitions only; device/rendered/native acceptance is separate. */
@RunWith(AndroidJUnit4::class)
class ServiceConfigurationControllerTest {
    private fun spec(reference:String="example/service@1.0.0")=JSONObject()
        .put("artifact_ref",reference).put("endpoint_id","local").put("command","serve").toString()
    private class Backend:ConfigurationBackend {
        override var current=true
        var revision=7uL
        var writes=0
        var volumeWrites=0
        var groupId:String?=null
        var failure:Exception?=null
        var load:CompletableDeferred<String>?=null
        var write:CompletableDeferred<String>?=null
        var volume:CompletableDeferred<String>?=null
        override suspend fun inspect(id:String):String=load?.await()?:JSONObject()
            .put("id",id).put("version",revision.toLong()).put("state","exited")
            .put("spec",JSONObject().put("artifact_ref","example/service@1.0.0").put("command","serve")).toString()
        override suspend fun group(id:String):String?=groupId
        override suspend fun create(spec:String):String {
            writes++;failure?.let{throw it};return write?.await()?:"{}"
        }
        override suspend fun configure(id:String,version:ULong,spec:String):String {
            check(version==revision){"Stale configuration"};return create(spec)
        }
        override suspend fun createVolume(name:String):String {
            volumeWrites++;failure?.let{throw it};return volume?.await()?:JSONObject().put("name",name).toString()
        }
    }
    private fun editor(block:(ServiceConfigurationController,Backend)->Unit){
        val scope=CoroutineScope(SupervisorJob()+Dispatchers.Unconfined)
        val backend=Backend()
        val controller=ServiceConfigurationController(scope,{backend},{})
        try{block(controller,backend)}finally{controller.close();scope.cancel()}
    }
    @Test fun staleCallbacksAndLateLoadCannotOverwriteTheSelectedApplication()=editor{editor,backend->
        backend.load=CompletableDeferred()
        editor.edit("old");val oldEpoch=editor.state.value.epoch
        editor.create(spec("example/new@2.0.0"));val selected=editor.state.value
        backend.load!!.complete("{}")
        editor.change(oldEpoch){it.copy(name="wrong")};editor.dismiss(oldEpoch);editor.raw(oldEpoch,"wrong")
        assertEquals(selected,editor.state.value)
        assertEquals("example/new@2.0.0",editor.state.value.reference)
    }
    @Test fun requiredSettingsOpenTheExistingVersionedEditorOnExecution() = editor { editor, backend ->
        editor.edit("wp3", initialTab = 1)
        val state = editor.state.value
        assertEquals("wp3", state.instanceId)
        assertEquals(backend.revision, state.version)
        assertEquals(1, state.tab)
        assertEquals(0, backend.writes)
        editor.reload(state.epoch)
        assertEquals(1, editor.state.value.tab)
    }
    @Test fun rejectedLossyConversionPreservesPrivateRowsAndRawIdentityCannotChange()=editor{editor,backend->
        editor.create(spec());val epoch=editor.state.value.epoch
        editor.change(epoch){it.copy(environment=listOf(EnvironmentDraft("KEY","first"),EnvironmentDraft("KEY","PRIVATE_MARKER")))}
        editor.tab(epoch,3)
        assertEquals(1,editor.state.value.tab)
        assertEquals(ConfigurationField.Environment,editor.state.value.field)
        assertEquals(2,editor.state.value.draft!!.environment.size)
        assertFalse(editor.state.value.toString().contains("PRIVATE_MARKER"))
        editor.change(epoch){it.copy(environment=emptyList())};editor.tab(epoch,3)
        editor.raw(epoch,spec("example/different@1.0.0"));editor.save(epoch)
        assertEquals(0,backend.writes);assertTrue(editor.state.value.open)
        assertEquals(ConfigurationField.Document,editor.state.value.field)
    }
    @Test fun staleNativeRevisionPreservesDraftUntilExplicitReload()=editor{editor,backend->
        editor.edit("app");val epoch=editor.state.value.epoch
        editor.change(epoch){it.copy(name="PRIVATE_DRAFT")};backend.revision++
        editor.save(epoch)
        assertEquals(ConfigurationProblem.Changed,editor.state.value.problem)
        assertEquals("PRIVATE_DRAFT",editor.state.value.draft!!.name)
        assertEquals(0,backend.writes)
        editor.reload(epoch)
        assertEquals(backend.revision,editor.state.value.version)
        assertNotEquals(epoch,editor.state.value.epoch)
        assertEquals("",editor.state.value.draft!!.name)
    }
    @Test fun disconnectedBackendCannotWriteAndReloadRetainsTheNewDraft()=editor{editor,backend->
        editor.create(spec());val epoch=editor.state.value.epoch
        editor.change(epoch){it.copy(name="unsaved")};backend.current=false;editor.save(epoch)
        assertEquals(ConfigurationProblem.Unavailable,editor.state.value.problem)
        assertEquals(0,backend.writes)
        backend.current=true;editor.reload(epoch)
        assertEquals("unsaved",editor.state.value.draft!!.name)
        editor.save(epoch);assertEquals(1,backend.writes);assertFalse(editor.state.value.open)
    }
    @Test fun pendingNativeSaveCannotBeDismissedOrDuplicatedAndItsAcknowledgementWins()=editor{editor,backend->
        backend.write=CompletableDeferred();editor.create(spec());val epoch=editor.state.value.epoch
        editor.save(epoch);editor.save(epoch);editor.dismiss(epoch);editor.edit("other")
        assertEquals(1,backend.writes);assertTrue(editor.state.value.saving)
        backend.current=false;backend.write!!.complete("{}")
        assertFalse(editor.state.value.open);assertNull(editor.state.value.draft)
    }
    @Test fun volumeIsConnectedOnlyAfterAcknowledgementAndSurvivesDraftDiscard()=editor{editor,backend->
        backend.volume=CompletableDeferred();editor.create(spec());val epoch=editor.state.value.epoch
        editor.change(epoch){it.copy(mounts=listOf(MountDraft("existing","/data",true)))}
        editor.volumeName(epoch,"new-data");editor.createVolume(epoch);editor.dismiss(epoch)
        assertTrue(editor.state.value.makingVolume);assertEquals(1,editor.state.value.draft!!.mounts.size)
        backend.volume!!.complete("{\"name\":\"new-data\"}")
        assertEquals("/data2",editor.state.value.draft!!.mounts.last().target)
        assertEquals(1,backend.volumeWrites)
        editor.dismiss(epoch);assertFalse(editor.state.value.open)
        // Discard has no delete operation, and never creates a guest.
        assertEquals(1,backend.volumeWrites);assertEquals(0,backend.writes)
    }
    @Test fun refreshFailureCannotTurnACommittedWriteIntoARetry(){
        val scope=CoroutineScope(SupervisorJob()+Dispatchers.Unconfined);val backend=Backend()
        val editor=ServiceConfigurationController(scope,{backend},{error("PRIVATE_REFRESH_MARKER")})
        try{
            editor.create(spec());editor.save(editor.state.value.epoch)
            assertFalse(editor.state.value.open);assertEquals(1,backend.writes)
        }finally{editor.close();scope.cancel()}
    }
    @Test fun failedVolumeCreationKeepsTheDraftAndExposesOnlyAFixedProblem()=editor{editor,backend->
        editor.create(spec());val epoch=editor.state.value.epoch
        editor.change(epoch){it.copy(name="PRIVATE_DRAFT")}
        editor.volumeName(epoch,"new-data");backend.failure=IllegalStateException("PRIVATE_PROVIDER_MARKER")
        editor.createVolume(epoch)
        assertEquals(ConfigurationProblem.Volume,editor.state.value.problem)
        assertEquals("PRIVATE_DRAFT",editor.state.value.draft!!.name)
        assertTrue(editor.state.value.draft!!.mounts.isEmpty())
        assertFalse(editor.state.value.toString().contains("PRIVATE_"))
        editor.change(epoch){it.copy(arguments=listOf("x".repeat(128*1024+1)))}
        assertTrue(editor.state.value.draft!!.arguments.isEmpty())
        assertEquals(ConfigurationField.Document,editor.state.value.field)
    }
}
