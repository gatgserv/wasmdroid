package dev.wasmdroid.app

import dev.wasmdroid.app.engine.LanEnableDispatch
import org.junit.Assert.*
import org.junit.Test
import java.util.concurrent.CompletableFuture
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger

/** Real competing threads exercise the actual worker dispatch gate. These
 * prove entry arbitration only, not native cleanup, binding or Wi-Fi I/O. */
class LanEnableDispatchTest {
    @Test fun revokeBeforeDelayedThreadDispatchMakesLateNativeEntryImpossible() {
        val gate = LanEnableDispatch()
        val enter = CountDownLatch(1)
        val nativeEntries = AtomicInteger()
        val completed = CompletableFuture<Boolean>()
        val worker = Thread {
            try {
                check(enter.await(3, TimeUnit.SECONDS))
                val dispatched = gate.dispatch()
                if (dispatched) nativeEntries.incrementAndGet()
                completed.complete(dispatched)
            } catch (failure:Throwable) { completed.completeExceptionally(failure) }
        }
        worker.start()
        try {
            gate.cancel()
            assertTrue(gate.cancelledBeforeDispatch)
            assertFalse(gate.allowed)
            enter.countDown()
            assertFalse(completed.get(3, TimeUnit.SECONDS))
            assertEquals(0, nativeEntries.get())
            assertFalse(gate.dispatch())
        } finally { enter.countDown(); worker.join(3000); assertFalse(worker.isAlive) }
    }

    @Test fun revokeAfterDispatchCannotClaimPreNativeClosureAndFencesLateBinder() {
        val gate = LanEnableDispatch()
        val dispatched = CountDownLatch(1)
        val inspectBinder = CountDownLatch(1)
        val binderAllowed = CompletableFuture<Boolean>()
        val worker = Thread {
            try {
                check(gate.dispatch())
                dispatched.countDown()
                check(inspectBinder.await(3, TimeUnit.SECONDS))
                binderAllowed.complete(gate.allowed)
            } catch (failure:Throwable) { binderAllowed.completeExceptionally(failure) }
        }
        worker.start()
        try {
            assertTrue(dispatched.await(3, TimeUnit.SECONDS))
            gate.cancel()
            assertFalse(gate.cancelledBeforeDispatch)
            assertFalse(gate.reserved)
            assertFalse(gate.dispatch())
            inspectBinder.countDown()
            assertFalse(binderAllowed.get(3, TimeUnit.SECONDS))
            // Only a real native receipt may close this dispatched grant.
            assertFalse(gate.cancelledBeforeDispatch)
        } finally { inspectBinder.countDown(); worker.join(3000); assertFalse(worker.isAlive) }
    }
}
