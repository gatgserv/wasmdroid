package dev.wasmdroid.app

import dev.wasmdroid.app.engine.LanStartIntent
import org.junit.Assert.*
import org.junit.Test
import java.util.concurrent.CountDownLatch
import java.util.concurrent.atomic.AtomicInteger

/** Pure lifetime intent checks; no Android callback, socket or guest executes. */
class LanStartIntentTest {
    @Test fun offBeforeReservationStillReachesExactlyThatReservationOnce() {
        val intent=LanStartIntent();val calls=AtomicInteger()
        intent.cancel();assertTrue(intent.cancelled)
        intent.onCancelled{calls.incrementAndGet()}.use{
            intent.cancel();assertEquals(1,calls.get())
        }
        assertThrows(IllegalStateException::class.java){intent.onCancelled{error("An old intent cannot own a new reservation")}}
        val current=LanStartIntent();val currentCalls=AtomicInteger()
        current.onCancelled{currentCalls.incrementAndGet()}
        intent.cancel();assertFalse(current.cancelled);assertEquals(0,currentCalls.get())
    }
    @Test fun concurrentOffAndReservationCannotLoseOrDuplicateTheCancellation() {
        repeat(32){
            val intent=LanStartIntent();val ready=CountDownLatch(1);val calls=AtomicInteger()
            val registering=Thread{ready.await();intent.onCancelled{calls.incrementAndGet()}}
            val stopping=Thread{ready.await();intent.cancel()}
            registering.isDaemon=true;stopping.isDaemon=true
            registering.start();stopping.start();ready.countDown();registering.join(2000);stopping.join(2000)
            assertFalse(registering.isAlive);assertFalse(stopping.isAlive)
            assertTrue(intent.cancelled);assertEquals(1,calls.get())
        }
    }
}
