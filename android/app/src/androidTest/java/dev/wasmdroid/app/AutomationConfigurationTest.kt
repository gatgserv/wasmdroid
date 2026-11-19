package dev.wasmdroid.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import dev.wasmdroid.app.ui.*
import kotlinx.coroutines.*
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

/** Codec/controller definitions only. No actual guest, service or UI runs. */
@RunWith(AndroidJUnit4::class)
class AutomationConfigurationTest {
    private fun spec()=JSONObject().put("artifact_ref","wasmer.io/example/automation@1.0.0")
        .put("endpoint_id","local").put("command","serve")
        .put("env",JSONObject().put("PRIVATE_ENV","PRIVATE_VALUE"))

    @Test fun legacyFlagsDefaultToOffAndNeverWithoutCreatingRunAuthority(){
        val draft=ServiceConfigurationDraft.fromSpecification(spec().toString())
        assertFalse(draft.startOnBoot);assertEquals(AutomaticRestartPolicy.Never,draft.restartPolicy)
        val wire=JSONObject(draft.specification())
        assertFalse(wire.getBoolean("start_on_boot"));assertEquals("no",wire.getString("restart_policy"))
        assertFalse(wire.has("desired_run"));assertFalse(wire.has("generation"));assertFalse(wire.has("start"))
    }

    @Test fun everyPolicyAndBootCombinationRoundTripsIndependentlyWithoutLosingPrivateFields(){
        for(policy in AutomaticRestartPolicy.entries)for(boot in listOf(false,true)){
            val source=spec().put("restart_policy",policy.wire).put("start_on_boot",boot)
                .put("resources",JSONObject().put("memory_max_bytes",123456))
            val draft=ServiceConfigurationDraft.fromSpecification(source.toString())
            assertEquals(policy,draft.restartPolicy);assertEquals(boot,draft.startOnBoot)
            val wire=JSONObject(draft.specification());assertEquals(policy.wire,wire.getString("restart_policy"));assertEquals(boot,wire.getBoolean("start_on_boot"))
            assertEquals("PRIVATE_VALUE",wire.getJSONObject("env").getString("PRIVATE_ENV"))
            assertEquals(123456,wire.getJSONObject("resources").getInt("memory_max_bytes"))
            val again=ServiceConfigurationDraft.fromSpecification(wire.toString())
            assertEquals(policy,again.restartPolicy);assertEquals(boot,again.startOnBoot)
            assertFalse(draft.toString().contains("PRIVATE_"))
        }
    }

    @Test fun malformedOrUnknownAutomationIsRejectedWithoutCoercionOrPrivateErrorValues(){
        val invalid=listOf(
            spec().put("start_on_boot","true"),spec().put("start_on_boot",1),spec().put("start_on_boot",JSONObject.NULL),
            spec().put("restart_policy","on-failure"),spec().put("restart_policy","PRIVATE_POLICY"),
            spec().put("restart_policy",true),spec().put("restart_policy",JSONObject.NULL),
        )
        invalid.forEach{source->
            val failure=assertThrows(ConfigurationInputError::class.java){ServiceConfigurationDraft.fromSpecification(source.toString())}
            assertEquals(ConfigurationField.Automation,failure.field);assertFalse(failure.message!!.contains("PRIVATE_"))
        }
    }

    private class Backend:ConfigurationBackend {
        override var current=true
        var revision=7uL
        var saved=JSONObject().put("artifact_ref","wasmer.io/example/automation@1.0.0").put("endpoint_id","local").put("command","serve").toString()
        var groupId:String?=null
        var membership:CompletableDeferred<String?>?=null
        var writes=0;var creates=0;var volumes=0
        var received:String?=null
        override suspend fun inspect(id:String):String=JSONObject().put("id",id).put("version",revision.toLong()).put("state","exited").put("spec",JSONObject(saved)).toString()
        override suspend fun group(id:String):String?=membership?.await()?:groupId
        override suspend fun create(spec:String):String{creates++;received=spec;return "{}"}
        override suspend fun configure(id:String,version:ULong,spec:String):String{
            check(version==revision){"Stale configuration"};writes++;received=spec;saved=spec;revision++;return "{}"
        }
        override suspend fun createVolume(name:String):String{volumes++;return JSONObject().put("name",name).toString()}
    }
    private fun editor(test:(ServiceConfigurationController,Backend)->Unit){
        val scope=CoroutineScope(SupervisorJob()+Dispatchers.Unconfined);val backend=Backend()
        val editor=ServiceConfigurationController(scope,{backend},{})
        try{test(editor,backend)}finally{editor.close();scope.cancel()}
    }

    @Test fun saveUsesTheCapturedInstanceRevisionAndOnlyTheConfigurationMutation()=editor{editor,backend->
        editor.edit("app");val epoch=editor.state.value.epoch
        editor.change(epoch){it.copy(startOnBoot=true,restartPolicy=AutomaticRestartPolicy.Always)}
        assertEquals(0,backend.writes);assertEquals(0,backend.creates)
        editor.save(epoch)
        assertEquals(1,backend.writes);assertEquals(0,backend.creates);assertEquals(0,backend.volumes)
        val received=JSONObject(backend.received!!);assertTrue(received.getBoolean("start_on_boot"));assertEquals("always",received.getString("restart_policy"))
        assertFalse(received.has("desired_run"));assertFalse(editor.state.value.open)
    }

    @Test fun conflictKeepsAutomationDraftUntilReloadAndDiscardWritesNothing()=editor{editor,backend->
        editor.edit("app");val epoch=editor.state.value.epoch
        editor.change(epoch){it.copy(startOnBoot=true,restartPolicy=AutomaticRestartPolicy.OnFailure)}
        backend.revision++;editor.save(epoch)
        assertEquals(ConfigurationProblem.Changed,editor.state.value.problem)
        assertTrue(editor.state.value.draft!!.startOnBoot);assertEquals(AutomaticRestartPolicy.OnFailure,editor.state.value.draft!!.restartPolicy)
        assertEquals(0,backend.writes)
        editor.reload(epoch);val refreshed=editor.state.value
        val refreshedDraft=requireNotNull(refreshed.draft)
        assertNotEquals(epoch,refreshed.epoch);assertFalse(refreshedDraft.startOnBoot);assertEquals(AutomaticRestartPolicy.Never,refreshedDraft.restartPolicy)
        editor.change(refreshed.epoch){it.copy(startOnBoot=true)};editor.dismiss(refreshed.epoch)
        assertFalse(editor.state.value.open);assertEquals(0,backend.writes)
    }

    @Test fun actualGroupOwnershipBlocksFormJsonSaveAndVolumeCreationWithoutUsingLabels()=editor{editor,backend->
        backend.groupId="actual-group";editor.edit("app");val epoch=editor.state.value.epoch
        assertEquals("actual-group",editor.state.value.groupId)
        editor.change(epoch){it.copy(startOnBoot=true,restartPolicy=AutomaticRestartPolicy.Always)}
        assertFalse(editor.state.value.draft!!.startOnBoot)
        editor.tab(epoch,3);val before=editor.state.value.raw;editor.raw(epoch,"PRIVATE_RAW_OVERRIDE")
        assertEquals(before,editor.state.value.raw)
        editor.save(epoch);editor.createVolume(epoch)
        assertEquals(ConfigurationProblem.Unsupported,editor.state.value.problem)
        assertEquals(0,backend.writes);assertEquals(0,backend.volumes)
    }

    @Test fun groupTransitionIsRecheckedBeforeSaveAndBeforeCreatingAnyVolume()=editor{editor,backend->
        editor.edit("app");val epoch=editor.state.value.epoch
        editor.change(epoch){it.copy(startOnBoot=true)};backend.membership=CompletableDeferred()
        editor.save(epoch);assertTrue(editor.state.value.saving)
        backend.membership!!.complete("new-group")
        assertEquals("new-group",editor.state.value.groupId);assertEquals(ConfigurationProblem.Unsupported,editor.state.value.problem)
        assertEquals(0,backend.writes)
        backend.membership=null;backend.groupId=null;editor.reload(epoch)
        val fresh=editor.state.value.epoch;editor.volumeName(fresh,"new-data");backend.groupId="new-group"
        editor.createVolume(fresh);assertEquals(0,backend.volumes);assertEquals(ConfigurationProblem.Unsupported,editor.state.value.problem)
    }

    @Test fun disconnectDuringMembershipReadCannotWriteAndPrivateDraftSurvives()=editor{editor,backend->
        editor.edit("app");val epoch=editor.state.value.epoch
        editor.change(epoch){it.copy(startOnBoot=true,restartPolicy=AutomaticRestartPolicy.Always)}
        backend.membership=CompletableDeferred();editor.save(epoch);backend.current=false
        backend.membership!!.complete(null)
        assertEquals(ConfigurationProblem.Unavailable,editor.state.value.problem);assertEquals(0,backend.writes)
        assertTrue(editor.state.value.draft!!.startOnBoot);assertEquals(AutomaticRestartPolicy.Always,editor.state.value.draft!!.restartPolicy)
    }
    @Test fun newApplicationReloadAndFormJsonConversionKeepExplicitChoicesUntilOneCreate()=editor{editor,backend->
        editor.create(spec().toString());val epoch=editor.state.value.epoch
        editor.change(epoch){it.copy(startOnBoot=true,restartPolicy=AutomaticRestartPolicy.OnFailure)}
        backend.current=false;editor.save(epoch);assertEquals(0,backend.creates)
        backend.current=true;editor.reload(epoch)
        assertTrue(editor.state.value.draft!!.startOnBoot);assertEquals(AutomaticRestartPolicy.OnFailure,editor.state.value.draft!!.restartPolicy)
        editor.tab(epoch,3);val raw=JSONObject(editor.state.value.raw)
        assertTrue(raw.getBoolean("start_on_boot"));assertEquals("on_failure",raw.getString("restart_policy"))
        editor.tab(epoch,0);editor.save(epoch)
        assertEquals(1,backend.creates);assertEquals(0,backend.writes);assertFalse(editor.state.value.open)
    }
    @Test fun delayedSingleFieldEventsCannotOverwriteOtherDraftEditsOrTheIndependentFlag()=editor{editor,backend->
        editor.edit("app");val epoch=editor.state.value.epoch
        // These callbacks may outlive the composition snapshot that rendered
        // the controls; each event applies only its own field to current state.
        val delayedBoot:(Boolean)->Unit={value->editor.change(epoch){it.copy(startOnBoot=value)}}
        val delayedPolicy:(AutomaticRestartPolicy)->Unit={value->editor.change(epoch){it.copy(restartPolicy=value)}}
        editor.change(epoch){it.copy(name="new name",environment=listOf(EnvironmentDraft("PRIVATE_ENV","PRIVATE_VALUE")))}
        delayedBoot(true)
        editor.change(epoch){it.copy(arguments=listOf("new argument"))}
        delayedPolicy(AutomaticRestartPolicy.OnFailure)
        val current=requireNotNull(editor.state.value.draft)
        assertEquals("new name",current.name);assertEquals("PRIVATE_VALUE",current.environment.single().value)
        assertEquals(listOf("new argument"),current.arguments)
        assertTrue(current.startOnBoot);assertEquals(AutomaticRestartPolicy.OnFailure,current.restartPolicy)
        assertEquals(0,backend.writes)
    }
}
