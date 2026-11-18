package dev.wasmdroid.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import dev.wasmdroid.app.ui.*
import kotlinx.coroutines.*
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

/** Deterministic controller definitions. No WebView/provider/device work runs. */
@RunWith(AndroidJUnit4::class)
class BrowserDataControllerTest {
    private companion object {
        val first = "wasmdroid-" + "a".repeat(64)
        val replacement = "wasmdroid-" + "b".repeat(64)
        fun frame(token: String = first, revision: Long = 0, active: Int = 0, outcome: String = "preserved", supported: Boolean = true): String =
            JSONObject().put("profile_token", token).put("revision", revision).put("active_views", active)
                .put("status", outcome).put("pending_erasure", outcome in setOf("clearing", "failed", "interrupted"))
                .put("capabilities", JSONObject().put("isolated_profiles", true).put("profile_data_erasure", supported)).toString()
    }
    private class Backend : BrowserDataBackend {
        override var current = true
        var json = frame()
        var load: CompletableDeferred<String>? = null
        var acknowledgement: CompletableDeferred<String>? = null
        var failure: Exception? = null
        var attempts = 0; var mutations = 0
        val selections = mutableListOf<Pair<Long, String>>()
        override suspend fun state() = load?.await() ?: json
        override suspend fun erase(revision: Long, profileToken: String): String {
            attempts++; selections += revision to profileToken
            val actual = JSONObject(json)
            check(profileToken == actual.getString("profile_token") && revision == actual.getLong("revision")) { "Browser profile changed" }
            failure?.let { throw it }
            mutations++
            return acknowledgement?.await() ?: frame(profileToken, revision + 2, outcome = "cleared").also { json = it }
        }
    }
    private fun fixture(block: (BrowserDataController, Backend) -> Unit) {
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)
        val backend = Backend(); val controller = BrowserDataController(scope, backend)
        try { block(controller, backend) } finally { controller.close(); scope.cancel() }
    }

    @Test fun inspectionPreservesDataAndLivingViewsOrUnsupportedProvidersCannotConfirm() = fixture { controller, backend ->
        controller.refresh(); assertEquals(BrowserDataStatus.Available, controller.state.value.snapshot!!.status)
        assertEquals(0, backend.attempts)
        backend.json = frame(active = 1); controller.refresh(); controller.requestConfirmation()
        assertEquals(BrowserDataStatus.InUse, controller.state.value.snapshot!!.status); assertNull(controller.state.value.confirmation)
        backend.json = frame(supported = false); controller.refresh(); controller.requestConfirmation()
        assertEquals(BrowserDataStatus.Unsupported, controller.state.value.snapshot!!.status); assertNull(controller.state.value.confirmation)
        assertEquals(0, backend.mutations)
    }

    @Test fun erasureRequiresExplicitConfirmationAndDismissKeepsData() = fixture { controller, backend ->
        controller.refresh(); controller.confirm(BrowserEraseConfirmation(0, first)); assertEquals(0, backend.attempts)
        controller.requestConfirmation(); assertNotNull(controller.state.value.confirmation)
        val dismissed = controller.state.value.confirmation!!
        assertEquals(0, backend.attempts)
        controller.dismissConfirmation(); controller.confirm(dismissed); assertEquals(0, backend.attempts)
        controller.requestConfirmation(); controller.confirm(controller.state.value.confirmation!!)
        assertEquals(listOf(0L to first), backend.selections); assertTrue(controller.state.value.acknowledged)
    }

    @Test fun aRecreatedActualInstanceWithSameZeroRevisionCannotConsumeAnOldConfirmation() = fixture { controller, backend ->
        controller.refresh(); controller.requestConfirmation()
        val shown = controller.state.value.confirmation!!
        backend.json = frame(replacement, 0)
        controller.confirm(shown)
        assertEquals(1, backend.attempts); assertEquals(0, backend.mutations)
        assertEquals(BrowserDataProblem.Changed, controller.state.value.problem)
        assertNull(controller.state.value.confirmation); assertFalse(controller.state.value.acknowledged)
        controller.requestConfirmation(); assertNull(controller.state.value.confirmation)
        controller.reload(); controller.requestConfirmation()
        assertEquals(replacement, controller.state.value.confirmation!!.profileToken)
    }

    @Test fun aNewRevisionInvalidatesTheOpenConfirmationBeforeAnyWrite() = fixture { controller, backend ->
        controller.refresh(); controller.requestConfirmation()
        val shown = controller.state.value.confirmation!!
        backend.json = frame(revision = 2); controller.refresh(); controller.confirm(shown)
        assertEquals(0, backend.attempts); assertNull(controller.state.value.confirmation)
        assertEquals(BrowserDataProblem.Changed, controller.state.value.problem)
    }

    @Test fun pollingClearedDataCannotFabricateThePendingProviderAcknowledgement() = fixture { controller, backend ->
        backend.acknowledgement = CompletableDeferred()
        controller.refresh(); controller.requestConfirmation(); val shown = controller.state.value.confirmation!!; controller.confirm(shown)
        controller.confirm(shown); assertEquals(1, backend.mutations)
        backend.json = frame(revision = 2, outcome = "cleared"); controller.refresh()
        assertTrue(controller.state.value.clearing); assertFalse(controller.state.value.acknowledged)
        backend.acknowledgement!!.complete("{}")
        assertFalse(controller.state.value.clearing); assertTrue(controller.state.value.acknowledged)
    }

    @Test fun aLateReadCannotOverwriteTheOwnerStateReloadedAfterAcknowledgement() = fixture { controller, backend ->
        backend.acknowledgement = CompletableDeferred()
        controller.refresh(); controller.requestConfirmation(); controller.confirm(controller.state.value.confirmation!!)
        val stale = CompletableDeferred<String>(); backend.load = stale; controller.refresh()
        backend.load = null; backend.json = frame(revision = 2, outcome = "cleared")
        backend.acknowledgement!!.complete("{}")
        stale.complete(frame(revision = 1, outcome = "clearing"))
        assertTrue(controller.state.value.snapshot!!.cleared); assertTrue(controller.state.value.acknowledged)
        assertFalse(controller.state.value.reading); assertFalse(controller.state.value.clearing)
    }

    @Test fun anOldProfileAcknowledgementCannotClaimThatAReplacementWasCleared() = fixture { controller, backend ->
        backend.acknowledgement = CompletableDeferred()
        controller.refresh(); controller.requestConfirmation(); controller.confirm(controller.state.value.confirmation!!)
        backend.json = frame(replacement, 0)
        backend.acknowledgement!!.complete("{}")
        assertEquals(replacement, controller.state.value.snapshot!!.profileToken)
        assertEquals(BrowserDataProblem.Changed, controller.state.value.problem)
        assertFalse(controller.state.value.acknowledged)
    }

    @Test fun disconnectDropsConfirmationAndClosingTheViewCannotCancelProviderWork() = fixture { controller, backend ->
        controller.refresh(); controller.requestConfirmation(); val shown = controller.state.value.confirmation!!
        backend.current = false; controller.confirm(shown)
        assertEquals(0, backend.attempts); assertNull(controller.state.value.confirmation)
        backend.current = true; controller.reload(); controller.requestConfirmation()
        backend.acknowledgement = CompletableDeferred(); controller.confirm(controller.state.value.confirmation!!); controller.close()
        assertFalse(backend.acknowledgement!!.isCancelled)
        backend.acknowledgement!!.complete(frame(revision = 2, outcome = "cleared"))
        assertFalse(controller.state.value.acknowledged)
    }

    @Test fun interruptedErasureRetriesOnlyAfterAnotherExplicitConfirmationAndPrivateStateStaysPrivate() = fixture { controller, backend ->
        backend.json = frame(revision = 1, outcome = "interrupted")
        controller.refresh(); assertEquals(BrowserDataStatus.Interrupted, controller.state.value.snapshot!!.status)
        assertEquals(0, backend.attempts); controller.requestConfirmation()
        assertFalse(controller.state.value.toString().contains(first))
        assertFalse(controller.state.value.snapshot.toString().contains(first))
        assertFalse(controller.state.value.confirmation.toString().contains(first))
        controller.confirm(controller.state.value.confirmation!!); assertEquals(1, backend.mutations); assertTrue(controller.state.value.acknowledged)
    }

    @Test fun malformedOwnerMetadataFailsClosedWithoutAnErasureAction() = fixture { controller, backend ->
        backend.json = frame().replace("\"preserved\"", "\"unrecognized\"")
        controller.refresh(); controller.requestConfirmation(); controller.confirm(BrowserEraseConfirmation(0, first))
        assertEquals(BrowserDataProblem.Unavailable, controller.state.value.problem)
        assertEquals(0, backend.attempts)
    }

    @Test fun aReadFromTheDisconnectedOwnerCannotOverwriteAReconnectedProfile() = fixture { controller, backend ->
        val old = CompletableDeferred<String>(); backend.load = old; controller.refresh()
        backend.current = false; controller.disconnected()
        backend.load = null; backend.current = true; backend.json = frame(replacement)
        controller.refresh(); old.complete(frame(first))
        assertEquals(replacement, controller.state.value.snapshot!!.profileToken)
        assertNull(controller.state.value.confirmation); assertEquals(0, backend.attempts)
    }

    @Test fun aLateOldDialogCallbackCannotConfirmANewReplacementPrompt() = fixture { controller, backend ->
        controller.refresh(); controller.requestConfirmation()
        val oldPrompt = controller.state.value.confirmation!!
        backend.json = frame(replacement, 0); controller.refresh(); controller.reload(); controller.requestConfirmation()
        val newPrompt = controller.state.value.confirmation!!
        controller.confirm(oldPrompt)
        assertEquals(0, backend.attempts); assertSame(newPrompt, controller.state.value.confirmation)
        controller.confirm(newPrompt)
        assertEquals(listOf(0L to replacement), backend.selections)
        assertEquals(1, backend.mutations); assertTrue(controller.state.value.acknowledged)
    }
}
