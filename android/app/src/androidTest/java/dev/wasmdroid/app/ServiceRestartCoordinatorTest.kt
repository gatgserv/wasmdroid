package dev.wasmdroid.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import dev.wasmdroid.app.engine.ServiceRestartCoordinator
import dev.wasmdroid.app.engine.ServiceRestartCoordinator.Completion
import dev.wasmdroid.app.engine.ServiceRestartCoordinator.Policy
import dev.wasmdroid.app.engine.ServiceRestartCoordinator.Record
import dev.wasmdroid.app.engine.ServiceRestartCoordinator.State
import dev.wasmdroid.app.engine.ServiceRestartCoordinator.Ticket
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

/** Deterministic actor/timer regression definitions; never launches a worker. */
@RunWith(AndroidJUnit4::class)
class ServiceRestartCoordinatorTest {
    private class Fixture {
        val timers = mutableListOf<Pair<Long, () -> Unit>>()
        val launches = mutableListOf<Ticket>()
        val failures = mutableListOf<Throwable>()
        val records = mutableMapOf<String, Record>()
        val counts = mutableListOf<Int>()
        val coordinator = ServiceRestartCoordinator(
            { delay, callback -> timers.add(delay to callback) },
            { records[it] },
            { launches.add(it) },
            { counts.add(coordinatorCount()) },
            { _, failure -> failures.add(failure) },
        )
        private fun coordinatorCount(): Int = coordinator.pendingCount
        fun record(policy: Policy = Policy.ON_FAILURE, state: State = State.EXITED,
                   id: String = "app", version: ULong = 3uL): Record =
            Record(id,version,state,policy,authorityRevision=1uL,desiredRun=true,epochMatches=true).also { records[id] = it }
        fun complete(record: Record = record(), generation: String = "first",
                     code: Int? = 1, owner: Boolean = false, millis: Long = 0): Boolean =
            coordinator.completed(Completion(record, generation, code, false, owner, true, true, millis))
        fun tick() { timers.removeAt(0).second() }
    }

    @Test fun observedDeathAndDurableTerminalCommitAreBothRequired() {
        val f = Fixture(); val record = f.record(); f.coordinator.started("app", "first")
        assertFalse(f.coordinator.completed(Completion(record,"first",1,false,false,false,true,0)))
        assertFalse(f.coordinator.completed(Completion(record,"first",1,false,false,true,false,0)))
        assertTrue(f.complete(record)); assertEquals(1,f.coordinator.pendingCount)
        assertFalse(f.complete(record)); assertEquals(1,f.timers.size)
        f.tick(); assertEquals(1,f.launches.size)
        assertEquals(1,f.coordinator.pendingCount) // Dispatch retains ownership reservation.
    }

    @Test fun policyAndOwnerStopDecideEligibilityWithoutFabricatedExitCodes() {
        for (policy in Policy.entries) for (code in listOf(0,1)) for (owner in listOf(false,true)) {
            val f=Fixture(); val record=f.record(policy); f.coordinator.started("app","first")
            val expected=!owner&&(policy==Policy.ALWAYS||(policy==Policy.ON_FAILURE&&code!=0))
            assertEquals(expected,f.complete(record,code=code,owner=owner))
        }
        val f=Fixture(); val dead=f.record(state=State.DEAD); f.coordinator.started("app","first")
        assertTrue(f.complete(dead,code=null)); f.tick()
        assertEquals(State.DEAD,f.launches.single().state)
    }

    @Test fun manualCancellationAndNewGenerationFenceLateTimerAndExit() {
        val f=Fixture(); f.coordinator.started("app","first"); f.complete()
        f.coordinator.cancel("app"); f.tick(); assertTrue(f.launches.isEmpty())
        f.coordinator.started("app","second")
        assertFalse(f.complete(generation="first")); assertEquals(0,f.coordinator.pendingCount)
        assertTrue(f.complete(generation="second"))
    }

    @Test fun changedRevisionPolicyOwnershipOrStackMembershipCannotStart() {
        val edits:List<(Record)->Record> = listOf(
            {it.copy(version=it.version+1uL)}, {it.copy(policy=Policy.NO)},
            {it.copy(owned=true)}, {it.copy(managed=true)},
        )
        edits.forEach { edit ->
            val f=Fixture(); val record=f.record(); f.coordinator.started("app","first"); f.complete(record)
            f.records["app"]=edit(record); f.tick()
            assertTrue(f.launches.isEmpty()); assertEquals(0,f.coordinator.pendingCount)
            assertEquals(1,f.failures.size)
        }
    }

    @Test fun deadReplacementKeepsReservationAndCanBeCancelledUsingEitherIdentity() {
        for (cancelId in listOf("app","replacement")) {
            val f=Fixture(); f.coordinator.started("app","first"); f.complete(f.record(state=State.DEAD),code=null)
            f.tick(); val old=f.launches.single(); val replacement=f.record(id="replacement",state=State.CREATED,version=0uL)
            val next=f.coordinator.replaced(old,replacement)
            assertFalse(f.coordinator.isPending(old)); assertTrue(f.coordinator.valid(next,replacement))
            assertEquals(1,f.coordinator.pendingCount)
            f.coordinator.cancel(cancelId)
            assertFalse(f.coordinator.valid(next,replacement)); assertEquals(0,f.coordinator.pendingCount)
            assertThrows(IllegalStateException::class.java) { f.coordinator.started("replacement","second",next) }
        }
    }

    @Test fun fastExitBackoffSurvivesAutomaticStartAndResetsAfterStableRun() {
        val f=Fixture(); f.coordinator.started("app","first"); f.complete()
        assertEquals(1_000L,f.timers.single().first); f.tick()
        f.coordinator.started("app","second",f.launches.last())
        assertEquals(0,f.coordinator.pendingCount)
        f.complete(generation="second"); assertEquals(2_000L,f.timers.single().first); f.tick()
        f.coordinator.started("app","third",f.launches.last())
        f.complete(generation="third",millis=ServiceRestartCoordinator.STABLE_RUN_MILLIS)
        assertEquals(1_000L,f.timers.single().first)
        assertEquals(60_000L,ServiceRestartCoordinator.backoffMillis(Int.MAX_VALUE))
    }

    @Test fun failedAdmissionAndStopAllReleasePendingWithoutLaunching() {
        val f=Fixture(); f.coordinator.started("app","first"); f.complete(); f.tick()
        f.coordinator.admissionFailed(f.launches.last(),IllegalStateException("No free worker"))
        assertEquals(0,f.coordinator.pendingCount); assertEquals(1,f.failures.size)
        f.coordinator.started("app","second"); f.complete(generation="second")
        f.coordinator.cancelAll(); f.tick(); assertEquals(1,f.launches.size)
    }
    @Test fun automaticTimeoutErrorQualifiesForOnFailureButOwnerStopStillSuppresses(){
        for(ownerStop in listOf(false,true)){
            val f=Fixture();val record=f.record();f.coordinator.started("app","first")
            assertEquals(!ownerStop,f.coordinator.completed(Completion(record,"first",0,true,ownerStop,true,true,0)))
        }
    }
    @Test fun nativeColdCandidatesRequireUnchangedDurableAuthorityAndNeverCreateFreshIntent(){
        val f=Fixture();val record=f.record(state=State.DEAD).copy(authorityRevision=9uL)
        f.records["app"]=record
        assertFalse(f.coordinator.recover(record.copy(desiredRun=false),"old-owner"))
        assertFalse(f.coordinator.recover(record.copy(epochMatches=false),"old-owner"))
        assertTrue(f.coordinator.recover(record,"old-owner"));assertFalse(f.coordinator.recover(record,"old-owner"))
        f.records["app"]=record.copy(authorityRevision=10uL,desiredRun=false)
        f.tick();assertTrue(f.launches.isEmpty());assertEquals(0,f.coordinator.pendingCount)
        val admitted=Fixture();admitted.records["app"]=record
        assertTrue(admitted.coordinator.recover(record,"old-owner"));admitted.tick()
        assertTrue(admitted.launches.single().recovery)
        assertEquals(9uL,admitted.launches.single().authorityRevision)
    }
    @Test fun absentAuthorityDefaultsCannotScheduleLiveRestartOrColdRecovery(){
        val f=Fixture();val legacy=Record("legacy",0uL,State.DEAD,Policy.ALWAYS)
        assertFalse(legacy.desiredRun);assertFalse(legacy.epochMatches);assertEquals(0uL,legacy.authorityRevision)
        f.coordinator.started("legacy","legacy-generation")
        assertFalse(f.coordinator.completed(Completion(legacy,"legacy-generation",null,true,false,true,true,0)))
        assertFalse(f.coordinator.recover(legacy,"legacy-generation"))
        assertEquals(0,f.coordinator.pendingCount);assertTrue(f.timers.isEmpty())
    }
}
