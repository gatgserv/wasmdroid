package dev.wasmdroid.app

import dev.wasmdroid.app.ui.*
import kotlinx.coroutines.*
import org.junit.Assert.*
import org.junit.Test

class NetworkPermissionControllerTest {
    private fun source(id:String="app",identity:String="birth",version:ULong=0u)=NetworkPermissionSource(
        id,identity,version,"Example","created",false,true,false,false,true)
    private class Backend(var value:NetworkPermissionSource):NetworkPermissionBackend {
        override var current=true
        var loading:CompletableDeferred<NetworkPermissionSource>?=null
        var saving:CompletableDeferred<Unit>?=null
        val saved=mutableListOf<Pair<Boolean,Boolean>>()
        override suspend fun source(id:String):NetworkPermissionSource=loading?.await()?:value
        override suspend fun save(source:NetworkPermissionSource,internet:Boolean,lan:Boolean){saved+=internet to lan;saving?.await()}
    }
    private suspend fun fixture(test:suspend(CoroutineScope)->Unit){
        val scope=CoroutineScope(SupervisorJob()+Dispatchers.Unconfined)
        try{test(scope)}finally{scope.cancel()}
    }
    @Test fun loadsActualIndependentPermissionsAndCancelDoesNotSave()=runBlocking{fixture{scope->
        val owner=Backend(source().copy(internet=true));val controller=NetworkPermissionController(scope,{owner},{})
        controller.edit("app");val epoch=controller.state.value.epoch
        assertTrue(controller.state.value.internet);assertFalse(controller.state.value.lan)
        controller.change(epoch,false,true);assertFalse(controller.state.value.internet);assertTrue(controller.state.value.lan)
        controller.dismiss(epoch);assertFalse(controller.state.value.open);assertTrue(owner.saved.isEmpty())
    }}
    @Test fun staleVersionNeverDispatchesMutation()=runBlocking{fixture{scope->
        val owner=Backend(source());val controller=NetworkPermissionController(scope,{owner},{})
        controller.edit("app");val epoch=controller.state.value.epoch;controller.change(epoch,true,false)
        owner.value=source(version=1u);controller.save(epoch)
        assertEquals(NetworkPermissionProblem.Changed,controller.state.value.problem);assertTrue(owner.saved.isEmpty())
    }}
    @Test fun rapidFieldCallbacksCannotRestoreTheOtherFieldsOldGrant()=runBlocking{fixture{scope->
        val owner=Backend(source().copy(internet=true));val controller=NetworkPermissionController(scope,{owner},{})
        controller.edit("app");val epoch=controller.state.value.epoch
        // Both callbacks came from the same previously composed frame.
        controller.internet(epoch,false);controller.lan(epoch,true);controller.save(epoch)
        assertEquals(listOf(false to true),owner.saved)
    }}
    @Test fun recreatedSameIdAndVersionCannotFollowOldIdentity()=runBlocking{fixture{scope->
        val owner=Backend(source());val controller=NetworkPermissionController(scope,{owner},{})
        controller.edit("app");val epoch=controller.state.value.epoch;controller.change(epoch,true,true)
        owner.value=source(identity="new-birth");controller.save(epoch)
        assertEquals(NetworkPermissionProblem.Changed,controller.state.value.problem);assertTrue(owner.saved.isEmpty())
    }}
    @Test fun ownerLossWhileLoadingCannotPublishSourceOrGrant()=runBlocking{fixture{scope->
        val owner=Backend(source());val gate=CompletableDeferred<NetworkPermissionSource>();owner.loading=gate
        val controller=NetworkPermissionController(scope,{owner},{})
        controller.edit("app");owner.current=false;gate.complete(source())
        assertNull(controller.state.value.source);assertEquals(NetworkPermissionProblem.Unavailable,controller.state.value.problem)
        controller.save(controller.state.value.epoch);assertTrue(owner.saved.isEmpty())
    }}
    @Test fun previousDialogLateReadCannotReplaceTheNewSelection()=runBlocking{fixture{scope->
        val old=Backend(source("old"));val gate=CompletableDeferred<NetworkPermissionSource>();old.loading=gate
        val fresh=Backend(source("new"));var selected:Backend=old
        val controller=NetworkPermissionController(scope,{selected},{})
        controller.edit("old");selected=fresh;controller.edit("new");gate.complete(source("old"))
        assertEquals("new",controller.state.value.source?.id);assertFalse(controller.state.value.loading)
    }}
    @Test fun saveOwnsSnapshotAndCannotBeDismissedOrRetargetedDuringNativeWork()=runBlocking{fixture{scope->
        val owner=Backend(source());val completion=CompletableDeferred<Unit>();owner.saving=completion
        var applied=0;val controller=NetworkPermissionController(scope,{owner},{applied++})
        controller.edit("app");val epoch=controller.state.value.epoch;controller.change(epoch,true,false);controller.save(epoch)
        assertTrue(controller.state.value.saving);controller.dismiss(epoch);controller.edit("another");controller.change(epoch,false,true)
        assertEquals("app",controller.state.value.id);assertTrue(controller.state.value.internet)
        completion.complete(Unit);assertFalse(controller.state.value.open);assertEquals(1,applied)
        assertEquals(listOf(true to false),owner.saved)
    }}
    @Test fun unsupportedManagedAndRunningSourcesCannotBypassDisabledControls()=runBlocking{fixture{scope->
        for(value in listOf(source().copy(supported=false),source().copy(managed=true),source().copy(status="running"))){
            val owner=Backend(value);val controller=NetworkPermissionController(scope,{owner},{})
            controller.edit("app");val epoch=controller.state.value.epoch;controller.change(epoch,true,true);controller.save(epoch)
            assertTrue(owner.saved.isEmpty());assertFalse(controller.state.value.internet)
        }
    }}
    @Test fun disabledNetworkingAllowsWithdrawingBothExistingFlagsWithoutAddingOne()=runBlocking{fixture{scope->
        val owner=Backend(source().copy(networkEnabled=false,internet=true,lan=true))
        val controller=NetworkPermissionController(scope,{owner},{})
        controller.edit("app");val epoch=controller.state.value.epoch
        controller.change(epoch,false,true);assertFalse(controller.state.value.internet)
        controller.change(epoch,true,true);assertFalse(controller.state.value.internet)
        controller.change(epoch,false,false);controller.save(epoch)
        assertEquals(listOf(false to false),owner.saved)
    }}
    @Test fun lostOwnerAfterActualMutationDoesNotClaimRollbackOrShowSaved()=runBlocking{fixture{scope->
        val owner=Backend(source());val completion=CompletableDeferred<Unit>();owner.saving=completion
        var applied=0;val controller=NetworkPermissionController(scope,{owner},{applied++})
        controller.edit("app");val epoch=controller.state.value.epoch;controller.change(epoch,false,true);controller.save(epoch)
        owner.current=false;completion.complete(Unit)
        assertEquals(listOf(false to true),owner.saved);assertEquals(0,applied)
        assertEquals(NetworkPermissionProblem.Unavailable,controller.state.value.problem)
        assertTrue(controller.state.value.open);assertFalse(controller.state.value.saving)
    }}
}
