package dev.wasmdroid.app

import dev.wasmdroid.app.engine.*
import java.util.concurrent.CompletableFuture
import org.junit.Assert.*
import org.junit.Test

class EgressRegrantCoordinatorTest{
    private class Backend:EgressRegrantBackend<Long,String>{
        var attached=EgressAttachment(EgressCurrent(1,1,false,false),true,false,false,false)
        var receipt=EgressReplacementStatus(2,2,false,false,false,false)
        var next=CompletableFuture.completedFuture(2L)
        var acquired=0;var begun=0;var dispatched=0;var cancelled=0
        var failBegin=false;var failDispatch=false;var afterBegin:(()->Unit)?=null
        val released=mutableListOf<Long>()
        override fun attachment()=attached
        override fun acquire()=next.also{acquired++}
        override fun epoch(capture:Long)=capture
        override fun release(capture:Long){released+=capture}
        override fun begin(capture:Long):String{begun++;if(failBegin)error("No admission");afterBegin?.invoke();return "retained"}
        override fun dispatch(operation:String){dispatched++;if(failDispatch)error("Actual admission result unknown")}
        override fun status(operation:String)=receipt
        override fun cancel(operation:String){cancelled++}
        fun closeOld(){attached=attached.copy(current=EgressCurrent(1,1,true,false),closed=true)}
        fun activate(){receipt=receipt.copy(admitted=true,terminal=true);attached=attached.copy(current=EgressCurrent(2,2,false,false),closed=false)}
    }
    @Test fun neverReplacesAnUndrainedOrUnreadyActualManager(){
        val b=Backend();val c=EgressRegrantCoordinator(b,1L);repeat(5){c.tick(it*250L)}
        assertEquals(0,b.begun);assertTrue(b.released.isEmpty())
        b.closeOld();b.attached=b.attached.copy(ready=false);c.tick(2_000)
        assertEquals(listOf(1L),b.released);assertEquals(0,b.begun)
        b.attached=b.attached.copy(ready=true);c.tick(2_250);assertEquals(1,b.begun)
    }
    @Test fun noNetworkFutureDoesNotBlockAndOnlyOneCaptureIsRequested(){
        val b=Backend();b.closeOld();b.next=CompletableFuture();val c=EgressRegrantCoordinator(b,1L)
        repeat(5){c.tick(it*250L)};assertEquals(1,b.acquired);assertEquals(0,b.begun)
        b.next.complete(2L);c.tick(1_500);assertEquals(1,b.begun)
    }
    @Test fun dispatchErrorRetainsOperationUntilActualNoAdmissionAndClosed(){
        val b=Backend();b.closeOld();b.failDispatch=true;val c=EgressRegrantCoordinator(b,1L)
        c.tick(0);c.tick(250);assertEquals(listOf(1L),b.released);assertEquals(1,b.begun)
        b.receipt=b.receipt.copy(terminal=true);c.tick(500);assertEquals(listOf(1L),b.released)
        b.receipt=b.receipt.copy(closed=true);c.tick(750);assertEquals(listOf(1L,2L),b.released)
        c.tick(1_000);assertEquals(1,b.begun)
    }
    @Test fun beginFailureIsNoAdmissionAndCanReleaseOnlyItsFreshCapture(){
        val b=Backend();b.closeOld();b.failBegin=true;val c=EgressRegrantCoordinator(b,1L)
        c.tick(0);assertEquals(listOf(1L,2L),b.released);assertEquals(0,b.dispatched)
        c.tick(250);assertEquals(1,b.begun)
    }
    @Test fun activeAdmissionPromotesOnlyMatchingNativeGrantAndNetwork(){
        val b=Backend();b.closeOld();val c=EgressRegrantCoordinator(b,1L);c.tick(0)
        b.receipt=b.receipt.copy(admitted=true,terminal=true);c.tick(250)
        assertEquals(listOf(1L),b.released);assertEquals(1,b.acquired)
        b.activate();c.tick(500);assertEquals(listOf(1L),b.released)
        b.attached=b.attached.copy(current=EgressCurrent(2,2,true,false),closed=true)
        b.next=CompletableFuture.completedFuture(3L);c.tick(750)
        assertEquals(listOf(1L,2L),b.released);assertEquals(2,b.begun)
    }
    @Test fun uncertainReceiptNeverAuthorizesSuccessorOrCaptureRelease(){
        val b=Backend();b.closeOld();val c=EgressRegrantCoordinator(b,1L);c.tick(0)
        b.receipt=b.receipt.copy(terminal=true,closed=true,uncertain=true);c.tick(250)
        b.receipt=b.receipt.copy(uncertain=false);c.tick(2_000)
        assertEquals(listOf(1L),b.released);assertEquals(1,b.begun)
        c.stop();assertEquals(1,b.cancelled)
    }
    @Test fun stopBetweenBeginAndDispatchCancelsRetainedTicket(){
        val b=Backend();b.closeOld();val c=EgressRegrantCoordinator(b,1L);b.afterBegin={c.stop()}
        c.tick(0);assertEquals(1,b.cancelled);assertEquals(1,b.dispatched)
        c.tick(2_000);assertEquals(1,b.begun)
    }
    @Test fun stopWhileCapturePendingNeverDispatchesAfterLateCompletion(){
        val b=Backend();b.closeOld();b.next=CompletableFuture();val c=EgressRegrantCoordinator(b,1L)
        c.tick(0);c.stop();b.next.complete(2L);c.tick(250);assertEquals(0,b.begun)
    }
}
