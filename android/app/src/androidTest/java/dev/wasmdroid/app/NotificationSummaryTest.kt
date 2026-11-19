package dev.wasmdroid.app

import dev.wasmdroid.app.engine.NotificationSummary
import dev.wasmdroid.app.engine.NotificationSummary.ApplicationKey
import dev.wasmdroid.app.engine.NotificationSummary.ApplicationWork
import dev.wasmdroid.app.engine.NotificationSummary.Kind
import dev.wasmdroid.app.engine.NotificationSummary.Phase
import org.junit.Assert.*
import org.junit.Test

/** Pure projection definitions: no Android service, native owner or guest runs. */
class NotificationSummaryTest {
    private fun standalone(id: String) = ApplicationKey(Kind.Standalone, id)
    private fun group(id: String) = ApplicationKey(Kind.Group, id)
    private fun work(key: ApplicationKey, phase: Phase) = ApplicationWork(key, phase)

    @Test fun emptySnapshotReportsNoApplicationsOrBackgroundWork() {
        val summary = NotificationSummary.summarize(emptyList(), false)
        assertEquals(0, summary.totalApplications)
        assertEquals(0, summary.runningApplications)
        assertEquals(0, summary.startingApplications)
        assertEquals(0, summary.stoppingApplications)
        assertFalse(summary.backgroundWork)
    }

    @Test fun backgroundOnlyWorkDoesNotBecomeAnApplication() {
        val summary = NotificationSummary.summarize(emptyList(), true)
        assertTrue(summary.backgroundWork)
        assertEquals(0, summary.totalApplications)
    }

    @Test fun multipleReadyWorkersInOneRealGroupCountOnce() {
        val actualGroup = group("native-group-a")
        val summary = NotificationSummary.summarize(List(4) { work(actualGroup, Phase.Running) }, false)
        assertEquals(1, summary.totalApplications)
        assertEquals(1, summary.runningApplications)
        assertEquals(0, summary.startingApplications)
    }

    @Test fun differentStandaloneNativeIdsRemainSeparateApplications() {
        val summary = NotificationSummary.summarize(listOf(
            work(standalone("native-app-a"), Phase.Running),
            work(standalone("native-app-b"), Phase.Running),
        ), false)
        assertEquals(2, summary.totalApplications)
        assertEquals(2, summary.runningApplications)
    }

    @Test fun groupAndStandaloneKeysCannotCollideOnTheSameIdText() {
        val summary = NotificationSummary.summarize(listOf(
            work(group("same-native-id-text"), Phase.Running),
            work(standalone("same-native-id-text"), Phase.Running),
        ), false)
        assertEquals(2, summary.totalApplications)
    }

    @Test fun independentGroupsDoNotMergeTheirWorkers() {
        val summary = NotificationSummary.summarize(listOf(
            work(group("native-group-a"), Phase.Running),
            work(group("native-group-a"), Phase.Running),
            work(group("native-group-b"), Phase.Running),
            work(group("native-group-b"), Phase.Running),
        ), false)
        assertEquals(2, summary.runningApplications)
        assertEquals(2, summary.totalApplications)
    }

    @Test fun groupWorkersAndPendingReservationAreOneStartingApplication() {
        val actualGroup = group("native-group-a")
        val summary = NotificationSummary.summarize(listOf(
            work(actualGroup, Phase.Running),
            work(actualGroup, Phase.Starting),
            work(actualGroup, Phase.Starting),
        ), false)
        assertEquals(1, summary.totalApplications)
        assertEquals(1, summary.startingApplications)
        assertEquals(0, summary.runningApplications)
        assertEquals(0, summary.stoppingApplications)
    }

    @Test fun restartAndBootReservationsForTheSameActualAppAreNotTwoApps() {
        val actualApp = standalone("native-app-a")
        val summary = NotificationSummary.summarize(listOf(
            work(actualApp, Phase.Starting),
            work(actualApp, Phase.Starting),
        ), false)
        assertEquals(1, summary.startingApplications)
        assertEquals(1, summary.totalApplications)
    }

    @Test fun stoppingWinsOverStartingAndReadyPartsRegardlessOfInputOrder() {
        val actualGroup = group("native-group-a")
        val phases = listOf(Phase.Running, Phase.Starting, Phase.Stopping)
        for (ordered in listOf(phases, phases.reversed(), listOf(Phase.Starting, Phase.Stopping, Phase.Running))) {
            val summary = NotificationSummary.summarize(ordered.map { work(actualGroup, it) }, false)
            assertEquals(1, summary.totalApplications)
            assertEquals(1, summary.stoppingApplications)
            assertEquals(0, summary.runningApplications)
            assertEquals(0, summary.startingApplications)
        }
    }

    @Test fun independentApplicationsKeepDisjointPhaseCountsAndBackgroundFact() {
        val summary = NotificationSummary.summarize(listOf(
            work(group("native-group-a"), Phase.Running),
            work(group("native-group-a"), Phase.Running),
            work(standalone("native-app-b"), Phase.Starting),
            work(standalone("native-app-c"), Phase.Stopping),
        ), true)
        assertEquals(3, summary.totalApplications)
        assertEquals(1, summary.runningApplications)
        assertEquals(1, summary.startingApplications)
        assertEquals(1, summary.stoppingApplications)
        assertTrue(summary.backgroundWork)
    }

    @Test fun repeatedObservationsAreIdempotentAndSnapshotRetainsNoInputList() {
        val actualGroup = group("native-group-a")
        val observations = mutableListOf(work(actualGroup, Phase.Starting), work(actualGroup, Phase.Running))
        val first = NotificationSummary.summarize(observations, false)
        assertEquals(first, NotificationSummary.summarize(observations + observations, false))
        observations.clear()
        observations.add(work(standalone("native-other"), Phase.Stopping))
        assertEquals(1, first.startingApplications)
        assertEquals(0, first.stoppingApplications)
        assertEquals(1, NotificationSummary.summarize(observations, false).stoppingApplications)
    }

    @Test fun logicalCountDoesNotEraseDistinctWorkerOwnershipReservations() {
        val actualGroup = group("native-group-a")
        val ownedReservations = listOf(work(actualGroup, Phase.Running), work(actualGroup, Phase.Running),
            work(actualGroup, Phase.Starting), work(actualGroup, Phase.Starting))
        val summary = NotificationSummary.summarize(ownedReservations, false)
        assertEquals(4, ownedReservations.size)
        assertEquals(1, summary.totalApplications)
        assertEquals(1, summary.startingApplications)
        // The original owned snapshot remains intact for the real lifetime gate.
        assertEquals(4, ownedReservations.size)
    }
}
