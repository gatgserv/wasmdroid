package dev.wasmdroid.app

import dev.wasmdroid.app.ui.*
import dev.wasmdroid.app.engine.packageFailure
import java.time.Instant
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

/** Admission policy definitions, not screenshots, Compose execution, HTTP or
 * Internet detection. Real owner/install validation remains independently required. */
class RegistrySearchPresentationTest {
    private fun accepted()=AppState(advanced=false,connected=true,registrySearch=RegistrySearchState(
        draftQuery="python",requestedQuery="python",resultsQuery="python",
        rows=listOf(RegistrySearchRow("wasmer.io/python/python@1.0.0","{}")),
        phase=RegistrySearchPhase.Ready,ownerConnected=true,
    ))
    private val reference="wasmer.io/python/python@1.0.0"

    @Test fun createServiceUsesTheExactSavedReleaseAndDisablesWhenItStopsBeingAvailable() {
        val image = JSONObject().put("reference", reference).put("commands", JSONArray().put("python")).toString()
        val otherVersion = JSONObject(image).put("reference", "wasmer.io/python/python@2.0.0").toString()
        val state = accepted().copy(runtimeAvailable = true, images = listOf(otherVersion, image))
        val spec = JSONObject(requireNotNull(downloadedPackageServiceSpec(state, image)))
        assertEquals(reference, spec.getString("artifact_ref"))
        assertEquals("python", spec.getString("command"))
        assertEquals("loopback", spec.getString("network"))
        assertEquals(0, spec.getJSONArray("ports").length())
        assertNull(downloadedPackageServiceSpec(state.copy(images = listOf(otherVersion)), image))
        assertNull(downloadedPackageServiceSpec(state.copy(busy = setOf("image:$reference")), image))
        assertNull(downloadedPackageServiceSpec(state.copy(connected = false), image))
        assertNull(downloadedPackageServiceSpec(state.copy(runtimeAvailable = false), image))
        val noCommand = JSONObject(image).put("commands", JSONArray()).toString()
        assertNull(downloadedPackageServiceSpec(state.copy(images = listOf(noCommand)), noCommand))
    }

    @Test fun createServiceUsesTheInstalledEntrypointBeforeSetupCommands() {
        for (entrypoint in listOf("run", "missing", null)) {
            val image = JSONObject().put("reference", reference)
                .put("commands", JSONArray(listOf("install", "run")))
                .put("labels", JSONObject().put("dev.wasmdroid.entrypoint", entrypoint))
                .toString()
            val state = accepted().copy(runtimeAvailable = true, images = listOf(image))
            val spec = JSONObject(requireNotNull(downloadedPackageServiceSpec(state, image)))
            assertEquals(if (entrypoint == "run") "run" else "install", spec.getString("command"))
        }
    }

    @Test fun onlyKnownLegacyWebcReleasesAreBlockedBeforeDownloading() {
        val original = accepted()
        for (version in listOf(1, 2, 3, null)) {
            val rows = original.registrySearch.rows.map { it.copy(webcVersion = version) }
            val actual = original.copy(registrySearch = original.registrySearch.copy(rows = rows))
            assertEquals(version == null || version == 3, registrySearchDownloadAllowed(actual, reference))
            assertEquals(version == null || version == 3, actual.registrySearch.visibleRows.isNotEmpty())
            assertEquals(rows, actual.registrySearch.copy(showIncompatible = true).visibleRows)
        }
        assertEquals(
            "registry_webc_version",
            packageFailure(IllegalStateException("unsupported capability: registry must provide WebC v3")).first,
        )
    }

    @Test fun testedVersionsLeadAcrossLoadedPagesWithRegistryOrderPreservedWithinEachGroup() {
        val baran = "wasmer.io/baran/static-web-server@1.0.5"
        val edoardo = "wasmer.io/edoardo/static-web-server@1.0.4"
        val python = "wasmer.io/python/python@3.13.20"
        val server = "wasmer.io/wasmer/static-web-server@2.44.0"
        val nextServerVersion = "wasmer.io/wasmer/static-web-server@2.44.1"
        val firstPage = listOf(baran, edoardo, python).map { RegistrySearchRow(it, "{}") }
        val secondPage = listOf(nextServerVersion, server).map { RegistrySearchRow(it, "{}") }

        assertEquals(listOf(python, baran, edoardo), orderRegistryResults(firstPage, RegistrySearchOrder.Registry).map { it.reference })
        assertEquals(
            listOf(python, server, baran, edoardo, nextServerVersion),
            orderRegistryResults(firstPage + secondPage, RegistrySearchOrder.Registry).map { it.reference },
        )
    }

    @Test fun newestReleaseKeepsTestedVersionsFirstAndSortsDatesAcrossPages() {
        val python = RegistrySearchRow(
            "wasmer.io/python/python@3.13.20", "{}", Instant.parse("2026-01-20T00:00:00Z"),
        )
        val server = RegistrySearchRow(
            "wasmer.io/wasmer/static-web-server@2.44.0", "{}", Instant.parse("2026-01-22T00:00:00Z"),
        )
        val laterUntestedVersion = RegistrySearchRow(
            "wasmer.io/wasmer/static-web-server@2.44.1", "{}", Instant.parse("2026-01-22T00:00:00.000001Z"),
        )
        val sameDate = laterUntestedVersion.copy(reference = "other")
        val undated = RegistrySearchRow("unknown", "{}")
        val firstPage = listOf(undated, python, laterUntestedVersion)
        val secondPage = listOf(server, sameDate)

        assertEquals(
            listOf(server, python, laterUntestedVersion, sameDate, undated),
            orderRegistryResults(firstPage + secondPage, RegistrySearchOrder.NewestRelease),
        )
    }

    @Test fun releaseDatesCompareAsInstantsAndMalformedValuesAreAbsent() {
        val instant = Instant.parse("2026-01-22T00:00:00.000001Z")
        assertEquals(instant, parseRegistryReleaseDate("2026-01-22T02:00:00.000001+02:00"))
        assertNull(parseRegistryReleaseDate(null))
        assertNull(parseRegistryReleaseDate(123))
        assertNull(parseRegistryReleaseDate("2026-13-22T00:00:00Z"))
        assertNull(parseRegistryReleaseDate("+1000000000-12-31T23:59:59.999999999Z"))
    }

    @Test fun knownPackageQueriesRequestOnlyTheExactTestedVersion() {
        assertEquals("wasmer.io/python/python@3.13.20", testedRegistryMatch("Python"))
        assertEquals(
            "wasmer.io/wasmer/static-web-server@2.44.0",
            testedRegistryMatch("static web server"),
        )
        assertNull(testedRegistryMatch("baran/static-web-server"))
        assertNull(testedRegistryMatch("wasmer/static-web-server@2.44.1"))
        assertNull(testedRegistryMatch("web"))
    }

    @Test fun downloadingRequiresDisplayedReferenceAndCurrentAcceptedQueryRatherThanRetainedFailureRows(){
        val state=accepted()
        assertTrue(registrySearchDownloadAllowed(state,reference))
        assertFalse(registrySearchDownloadAllowed(state,"wasmer.io/other/package@1.0.0"))
        for(phase in listOf(RegistrySearchPhase.LoadingFirst,RegistrySearchPhase.LoadingNext,
            RegistrySearchPhase.Failed,RegistrySearchPhase.Cancelled,RegistrySearchPhase.Unavailable)){
            assertFalse(registrySearchDownloadAllowed(state.copy(registrySearch=state.registrySearch.copy(phase=phase)),reference))
        }
        assertFalse(registrySearchDownloadAllowed(state.copy(registrySearch=state.registrySearch.copy(requestedQuery="new")),reference))
        // Draft edits alone do not erase or retag the last accepted submission.
        assertTrue(registrySearchDownloadAllowed(state.copy(registrySearch=state.registrySearch.copy(draftQuery="unsent")),reference))
    }
    @Test fun downloadAdmissionWorksInNormalModeAndRequiresBothOwnerSignalsAndCurrentPackageAvailability(){
        val state=accepted()
        assertFalse(registrySearchDownloadAllowed(state.copy(connected=false),reference))
        assertTrue(registrySearchDownloadAllowed(state,reference))
        assertTrue(registrySearchDownloadAllowed(state.copy(advanced=true),reference))
        assertFalse(registrySearchDownloadAllowed(state.copy(registrySearch=state.registrySearch.copy(ownerConnected=false)),reference))
        assertFalse(registrySearchDownloadAllowed(state.copy(packagesUnavailable=true),reference))
        assertFalse(registrySearchDownloadAllowed(state.copy(busy=setOf("pull:$reference")),reference))
        assertFalse(registrySearchDownloadAllowed(state.copy(packages=listOf(PackageTask("k","g","pull","public","publishing"))),reference))
    }
    @Test fun buttonAndImeSubmitShareOwnerAndLoadingAdmissionWithoutClaimingInternetAvailability(){
        val state=accepted()
        assertTrue(registrySearchSubmitAllowed(state))
        for(phase in listOf(RegistrySearchPhase.LoadingFirst,RegistrySearchPhase.LoadingNext))
            assertFalse(registrySearchSubmitAllowed(state.copy(registrySearch=state.registrySearch.copy(phase=phase))))
        assertFalse(registrySearchSubmitAllowed(state.copy(connected=false)))
        assertFalse(registrySearchSubmitAllowed(state.copy(registrySearch=state.registrySearch.copy(ownerConnected=false))))
        assertFalse(registrySearchSubmitAllowed(state.copy(registrySearch=state.registrySearch.copy(draftQuery=" "))))
        assertFalse(registrySearchSubmitAllowed(state.copy(registrySearch=state.registrySearch.copy(draftQuery="x".repeat(257)))))
        assertTrue(registrySearchSubmitAllowed(state.copy(registrySearch=state.registrySearch.copy(phase=RegistrySearchPhase.Failed,problem=RegistrySearchProblem.Failed))))
    }
}
