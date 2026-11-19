package dev.wasmdroid.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import dev.wasmdroid.app.ui.*
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

/** Presentation/admission definitions; no network or Compose rendering runs. */
@RunWith(AndroidJUnit4::class)
class CatalogUpdatesPresentationTest {
    private fun current() = AppState(connected = true, catalogUpdateConfiguration = "configured", catalogRemote = true, catalogFreshness = "current")

    @Test fun unconfiguredAndInvalidBuildsCannotStartRemoteRefresh() {
        val bundled = AppState(connected = true)
        assertEquals(CatalogUpdateStatus.NotConfigured, CatalogUpdatesView.from(bundled).status)
        assertFalse(CatalogUpdatesView.from(bundled).canRefresh)
        assertNull(CatalogUpdatesView.from(bundled).cancelGeneration)
        assertTrue(catalogInstallAllowed(bundled))
        assertEquals(CatalogUpdateStatus.InvalidConfiguration, CatalogUpdatesView.from(bundled.copy(catalogUpdateConfiguration = "invalid")).status)
    }

    @Test fun expiredClockAndReleaseFloorPauseRemoteInstallationButAllowExplicitRefresh() {
        for ((freshness, expected) in listOf("expired" to CatalogUpdateStatus.Expired, "clock_before_acceptance" to CatalogUpdateStatus.Clock, "below_minimum_sequence" to CatalogUpdateStatus.UpdateRequired)) {
            val state = current().copy(catalogFreshness = freshness)
            val presentation = CatalogUpdatesView.from(state)
            assertEquals(expected, presentation.status); assertTrue(presentation.canRefresh)
            assertFalse(presentation.installAllowed); assertFalse(catalogInstallAllowed(state))
        }
    }

    @Test fun cancellationCapturesAnActualGenerationAndDoesNotInventOneWhileAdmissionIsQueued() {
        val queued = current().copy(busy = setOf("catalog:refresh"))
        assertEquals(CatalogUpdateStatus.Refreshing, CatalogUpdatesView.from(queued).status)
        assertFalse(CatalogUpdatesView.from(queued).canRefresh); assertNull(CatalogUpdatesView.from(queued).cancelGeneration)
        val first = queued.copy(catalogRefreshing = true, catalogRefreshGeneration = "first")
        val captured = CatalogUpdatesView.from(first).cancelGeneration
        val replacement = first.copy(catalogRefreshGeneration = "replacement")
        assertEquals("first", captured); assertEquals("replacement", CatalogUpdatesView.from(replacement).cancelGeneration)
    }

    @Test fun failedUpdateRetainsUsableCurrentCatalogButDoesNotUnblockExpiredOrUnknownSnapshots() {
        val failed = current().copy(catalogRefreshOutcome = "failed")
        assertEquals(CatalogUpdateStatus.Failed, CatalogUpdatesView.from(failed).status)
        assertTrue(catalogInstallAllowed(failed))
        assertFalse(catalogInstallAllowed(failed.copy(catalogFreshness = "expired")))
        assertFalse(catalogInstallAllowed(failed.copy(catalogError = true)))
        assertFalse(catalogInstallAllowed(failed.copy(connected = false)))
        assertFalse(catalogInstallAllowed(failed.copy(catalogUpdateConfiguration = "invalid")))
        assertFalse(catalogInstallAllowed(failed.copy(catalogUpdateConfiguration = "disabled")))
    }
}
