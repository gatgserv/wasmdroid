package dev.wasmdroid.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import dev.wasmdroid.app.engine.ServiceLifetime
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

/** Main/actor orderings are explicit; no actual Service lifecycle is executed. */
@RunWith(AndroidJUnit4::class)
class ServiceLifetimeTest {
    @Test fun staleZeroCannotRetireAfterNewActorWorkWasReserved(){
        val lifetime=ServiceLifetime();lifetime.bound()
        val idle=lifetime.reserveIdle(true)!!
        lifetime.owned(1)
        var stopped=false
        assertFalse(lifetime.commit(idle){stopped=true});assertFalse(stopped)
        assertNull(lifetime.reserveIdle(false))
    }
    @Test fun unboundRetirementFencesLateAdmissionButBindingReopensSameSupervisor(){
        val lifetime=ServiceLifetime()
        val permit=lifetime.reserveIdle(true)!!
        assertTrue(permit.stopService);assertFalse(lifetime.admits())
        assertThrows(IllegalStateException::class.java){lifetime.beginBackground()}
        lifetime.bound()
        assertTrue(lifetime.admits());assertFalse(lifetime.commit(permit){fail("Stale stop")})
        lifetime.owned(1);assertTrue(lifetime.admits())
    }
    @Test fun aNewStartedServiceInvalidatesTheCapturedStopAndBackgroundWorkPreventsRetirement(){
        val lifetime=ServiceLifetime();val permit=lifetime.reserveIdle(true)!!
        lifetime.started();assertFalse(lifetime.commit(permit){fail("Old start ID")})
        lifetime.beginBackground();assertNull(lifetime.reserveIdle(true))
        lifetime.endBackground();val next=lifetime.reserveIdle(true)!!
        var stop=false;assertTrue(lifetime.commit(next){stop=it});assertTrue(stop)
        assertFalse(lifetime.admits())
    }
    @Test fun boundIdleOnlyRemovesForegroundAndDestructionPermanentlyFencesOldOwner(){
        val lifetime=ServiceLifetime();lifetime.bound();val permit=lifetime.reserveIdle(true)!!
        assertFalse(permit.stopService);assertTrue(lifetime.admits())
        assertTrue(lifetime.commit(permit){assertFalse(it)})
        lifetime.destroy();lifetime.started();lifetime.bound()
        assertFalse(lifetime.admits());assertNull(lifetime.reserveIdle(true))
    }
    @Test fun repeatedSameZeroNotificationCannotStrandAnUnboundRetirementPermit(){
        val lifetime=ServiceLifetime();val permit=lifetime.reserveIdle(true)!!
        lifetime.owned(0) // A queued background-finish/count notification.
        assertSame(permit,lifetime.reserveIdle(true))
        var stopped=false;assertTrue(lifetime.commit(permit){stopped=it});assertTrue(stopped)
        assertFalse(lifetime.admits())
    }
}
