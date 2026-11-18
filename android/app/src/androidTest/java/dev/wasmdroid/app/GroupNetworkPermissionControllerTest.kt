package dev.wasmdroid.app

import dev.wasmdroid.app.ui.*
import kotlinx.coroutines.*
import org.junit.Assert.*
import org.junit.Test

class GroupNetworkPermissionControllerTest{
    private fun source()=GroupPermissionSource("target","Example",1u,0u,listOf(GroupPermissionMember("web","birth","web",0u)),null,false,true,true,true)
    private class Review(override val source:GroupPermissionSource):GroupPermissionReview
    private class Backend(var value:GroupPermissionSource):GroupPermissionBackend{
        override var current=true
        var loading:CompletableDeferred<GroupPermissionReview>?=null
        var saving:CompletableDeferred<Unit>?=null
        var reviewed:GroupPermissionReview?=null;var saved:GroupPermissionReview?=null
        val choices=mutableListOf<Pair<Boolean,Boolean>>()
        override suspend fun review(target:String):GroupPermissionReview=(loading?.await()?:Review(value)).also{reviewed=it}
        override suspend fun save(review:GroupPermissionReview,internet:Boolean,lan:Boolean){saved=review;choices+=internet to lan;saving?.await()}
    }
    private suspend fun fixture(test:suspend(CoroutineScope)->Unit){val scope=CoroutineScope(SupervisorJob()+Dispatchers.Unconfined);try{test(scope)}finally{scope.cancel()}}
    @Test fun mixedRequiresExplicitChoiceAndSavesExactOpaqueReview()=runBlocking{fixture{scope->
        val b=Backend(source());val c=GroupNetworkPermissionController(scope,{b},{})
        c.edit("target");val epoch=c.state.value.epoch;c.save(epoch);assertTrue(b.choices.isEmpty())
        c.choose(epoch,true,false);c.choose(epoch,false,true);c.save(epoch)
        assertEquals(listOf(false to true),b.choices);assertSame(b.reviewed,b.saved);assertFalse(c.state.value.open)
    }}
    @Test fun cancelAndOldEpochCannotMutateNextDialog()=runBlocking{fixture{scope->
        val b=Backend(source());val c=GroupNetworkPermissionController(scope,{b},{})
        c.edit("target");val old=c.state.value.epoch;c.dismiss(old);c.edit("target")
        c.choose(old,true,true);c.save(old);assertNull(c.state.value.internet);assertTrue(b.choices.isEmpty())
    }}
    @Test fun runningUnsupportedAndLostOwnerCannotGrant()=runBlocking{fixture{scope->
        for(value in listOf(source().copy(canEdit=false),source().copy(supported=false))){
            val b=Backend(value);val c=GroupNetworkPermissionController(scope,{b},{})
            c.edit("target");val epoch=c.state.value.epoch;c.choose(epoch,true,true);c.save(epoch);assertTrue(b.choices.isEmpty())
        }
        val b=Backend(source().copy(internet=false));val c=GroupNetworkPermissionController(scope,{b},{})
        c.edit("target");b.current=false;c.save(c.state.value.epoch);assertTrue(b.choices.isEmpty())
    }}
    @Test fun saveCannotRetargetAndLostOwnerCannotClaimSuccess()=runBlocking{fixture{scope->
        val b=Backend(source().copy(internet=false));b.saving=CompletableDeferred();var applied=0
        val c=GroupNetworkPermissionController(scope,{b},{applied++});c.edit("target");val epoch=c.state.value.epoch
        c.save(epoch);c.dismiss(epoch);c.edit("new");assertEquals("target",c.state.value.target)
        b.current=false;b.saving!!.complete(Unit);assertEquals(0,applied)
        assertEquals(NetworkPermissionProblem.Unavailable,c.state.value.problem);assertTrue(c.state.value.open)
    }}
    @Test fun disabledNetworkAllowsWithdrawalButNeverGrant()=runBlocking{fixture{scope->
        val b=Backend(source().copy(internet=true,lan=true,networkEnabled=false));val c=GroupNetworkPermissionController(scope,{b},{})
        c.edit("target");val epoch=c.state.value.epoch;c.choose(epoch,true,false);c.choose(epoch,true,true)
        assertEquals(false,c.state.value.internet);c.choose(epoch,false,false);c.save(epoch)
        assertEquals(listOf(false to false),b.choices)
    }}
}
