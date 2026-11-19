package dev.wasmdroid.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import dev.wasmdroid.app.ui.*
import kotlinx.coroutines.*
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

/** Draft/authority races only; actual Compose, storage and provider tests differ. */
@RunWith(AndroidJUnit4::class)
class StackEditControllerTest {
    private fun StackEditController.editDocument(text:String)=edit(state.value.epoch,text)
    private fun StackEditController.reviewNow()=review(state.value.epoch,state.value.revision)
    private fun StackEditController.applyNow()=apply(state.value.epoch,state.value.preview?:"stale preview")
    private fun StackEditController.leaveNow()=leave(state.value.epoch)
    private fun StackEditController.reloadNow()=reload(state.value.epoch)
    private fun StackEditController.acceptNow()=acceptSwitch(state.value.epoch,state.value.pendingId?:"stale target")
    private fun StackEditController.cancelNow()=cancelSwitch(state.value.epoch)
    private fun StackEditController.discardNow()=discard(state.value.epoch)
    private open class Backend:StackEditBackend {
        var connected=true
        var version=1uL
        val applied=mutableListOf<Pair<String,String>>()
        override val current get()=connected
        override suspend fun source(id:String)="""{"id":"$id","version":$version,"document":"source-$version"}"""
        override suspend fun requirements(document:String)="[]"
        override suspend fun preview(id:String,version:ULong,document:String)="review:$id:$version:$document"
        override suspend fun apply(id:String,preview:String):String{applied.add(id to preview);return "{}"}
    }
    @Test fun delayedReviewCannotAuthorizeAnEditedDocument()=runBlocking {
        val scope=CoroutineScope(SupervisorJob()+Dispatchers.Unconfined)
        val late=CompletableDeferred<String>()
        val backend=object:Backend(){
            override suspend fun preview(id:String,version:ULong,document:String)=withContext(NonCancellable){late.await()}
        }
        val controller=StackEditController(scope,{backend},{})
        try{
            controller.open("group");controller.editDocument("old private draft");controller.reviewNow()
            assertTrue(controller.state.value.reviewing)
            controller.editDocument("new private draft")
            late.complete("old review");yield()
            assertNull(controller.state.value.preview)
            assertEquals("new private draft",controller.state.value.document)
            controller.applyNow();assertTrue(backend.applied.isEmpty())
        }finally{controller.close();scope.cancel()}
    }
    @Test fun durableApplyAcknowledgementSurvivesLeavingAndConnectionChangeWithoutDuplicate()=runBlocking {
        val scope=CoroutineScope(SupervisorJob()+Dispatchers.Unconfined)
        val committed=CompletableDeferred<String>()
        var notifications=0
        val backend=object:Backend(){
            override suspend fun apply(id:String,preview:String):String{applied.add(id to preview);return committed.await()}
        }
        val controller=StackEditController(scope,{backend},{notifications++})
        try{
            controller.open("group");controller.editDocument("private draft");controller.reviewNow();controller.applyNow()
            controller.leaveNow();backend.connected=false
            controller.acceptSwitch(controller.state.value.epoch,"another-group")
            assertTrue(controller.state.value.applying);assertFalse(controller.state.value.open)
            assertEquals("group",controller.state.value.id)
            controller.applyNow();assertEquals(1,backend.applied.size)
            committed.complete("{}");yield()
            assertTrue(controller.state.value.applied);assertFalse(controller.state.value.open)
            assertEquals("",controller.state.value.document);assertEquals("",controller.state.value.original)
            assertEquals(1,notifications);controller.applyNow();assertEquals(1,backend.applied.size)
        }finally{controller.close();scope.cancel()}
    }
    @Test fun staleApplyPreservesDraftButRequiresReloadAndANewReview()=runBlocking {
        val scope=CoroutineScope(SupervisorJob()+Dispatchers.Unconfined)
        val versions=mutableListOf<ULong>()
        val backend=object:Backend(){
            override suspend fun preview(id:String,version:ULong,document:String):String{versions.add(version);return super.preview(id,version,document)}
            override suspend fun apply(id:String,preview:String):String{throw IllegalStateException("stale member revision")}
        }
        val controller=StackEditController(scope,{backend},{})
        try{
            controller.open("group");controller.editDocument("private draft");controller.reviewNow();controller.applyNow()
            assertEquals(StackEditProblem.Changed,controller.state.value.problem)
            assertEquals("private draft",controller.state.value.document);assertNull(controller.state.value.preview)
            backend.version=2u;controller.reloadNow()
            assertEquals("source-2",controller.state.value.document);assertFalse(controller.state.value.dirty)
            controller.reviewNow();assertEquals(listOf(1uL,2uL),versions)
        }finally{controller.close();scope.cancel()}
    }
    @Test fun oldCallbacksCannotEditReloadedSourceOrApplyWhileSwitchIsPending()=runBlocking {
        val scope=CoroutineScope(SupervisorJob()+Dispatchers.Unconfined)
        val backend=Backend();val controller=StackEditController(scope,{backend},{})
        try{
            controller.open("first");controller.editDocument("private draft");controller.reviewNow()
            val oldEpoch=controller.state.value.epoch;val oldPreview=controller.state.value.preview!!
            controller.open("second");controller.apply(oldEpoch,oldPreview)
            assertTrue(backend.applied.isEmpty());assertEquals("first",controller.state.value.id)
            controller.acceptNow();assertEquals("second",controller.state.value.id)
            controller.edit(oldEpoch,"late first-group text")
            assertEquals("source-1",controller.state.value.document)
            val beforeReload=controller.state.value.epoch
            backend.version=2uL;controller.reloadNow()
            controller.edit(beforeReload,"late pre-reload text")
            controller.review(beforeReload,controller.state.value.revision)
            assertEquals("source-2",controller.state.value.document);assertNull(controller.state.value.preview)
            controller.reviewNow();controller.apply(oldEpoch,oldPreview)
            assertTrue(backend.applied.isEmpty())
        }finally{controller.close();scope.cancel()}
    }
    @Test fun oversizedEditAndSwitchCannotReuseReviewOrSilentlyDiscardPrivateDraft()=runBlocking {
        val scope=CoroutineScope(SupervisorJob()+Dispatchers.Unconfined)
        val backend=Backend();val controller=StackEditController(scope,{backend},{})
        try{
            controller.open("first");controller.editDocument("private draft");controller.reviewNow()
            assertNotNull(controller.state.value.preview)
            controller.editDocument("é".repeat(MAX_STACK_DOCUMENT_BYTES/2+1))
            assertEquals(StackEditProblem.Large,controller.state.value.problem)
            assertNull(controller.state.value.preview);assertNull(controller.state.value.requirements)
            controller.applyNow();assertTrue(backend.applied.isEmpty())
            controller.leaveNow();controller.open("second")
            assertEquals("second",controller.state.value.pendingId);assertEquals("private draft",controller.state.value.document)
            controller.cancelNow();assertEquals("first",controller.state.value.id)
            controller.open("second");controller.acceptNow()
            assertEquals("second",controller.state.value.id);assertFalse(controller.state.value.dirty)
            controller.discardNow();assertEquals("",controller.state.value.document);assertNull(controller.state.value.id)
        }finally{controller.close();scope.cancel()}
    }
}
