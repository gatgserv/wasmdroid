package dev.wasmdroid.app

import dev.wasmdroid.app.engine.EgressCaptureGate
import java.util.concurrent.CountDownLatch
import java.util.concurrent.atomic.AtomicInteger
import org.junit.Assert.*
import org.junit.Test

class EgressCaptureGateTest {
    @Test fun lossBeforeRevokerInstallationIsDeliveredOnceAndNeverResurrects(){
        var admitted=true;val gate=EgressCaptureGate{admitted};val calls=AtomicInteger()
        gate.invalidate();assertFalse(gate.install{calls.incrementAndGet()});assertEquals(1,calls.get())
        admitted=false;admitted=true;assertFalse(gate.current);gate.invalidate();assertEquals(1,calls.get())
    }
    @Test fun duplicateInstallCannotReplaceTheFirstNativeRevoker(){
        val gate=EgressCaptureGate{true};val first=AtomicInteger();val second=AtomicInteger()
        assertTrue(gate.install{first.incrementAndGet()});assertFalse(gate.install{second.incrementAndGet()})
        gate.invalidate();assertEquals(1,first.get());assertEquals(0,second.get())
    }
    @Test fun stopDuringInstallationFencesBeforeAnyPlatformWork(){
        var admitted=true;val gate=EgressCaptureGate{admitted};var revoked=false
        admitted=false;assertFalse(gate.install{revoked=true});assertTrue(revoked);assertFalse(gate.current)
    }
    @Test fun concurrentLossAndInstallationCannotLoseTheRevokeSignal(){
        repeat(20){
            val gate=EgressCaptureGate{true};val calls=AtomicInteger();val start=CountDownLatch(1)
            val install=Thread{start.await();gate.install{calls.incrementAndGet()}}
            val loss=Thread{start.await();gate.invalidate()}
            install.start();loss.start();start.countDown();install.join();loss.join()
            assertFalse(gate.current);assertEquals(1,calls.get())
        }
    }
    @Test fun failingObserverStillLeavesTheCapturePermanentlyRevoked(){
        val gate=EgressCaptureGate{true};assertTrue(gate.install{error("synthetic observer failure")})
        gate.invalidate();assertFalse(gate.current);gate.invalidate();assertFalse(gate.current)
    }
}
