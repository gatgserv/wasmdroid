package dev.wasmdroid.app

import dev.wasmdroid.app.ui.ServiceFailure
import dev.wasmdroid.app.ui.serviceErrorPreview
import dev.wasmdroid.app.ui.serviceFailure
import dev.wasmdroid.app.engine.CONFIRMED_OWNER_STOP
import dev.wasmdroid.app.engine.workerExitReason
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ServiceFailureTest {
    @Test fun terminalFailuresRemainVisibleUsingStoredExitInformation() {
        val nonzero = JSONObject("""{"state":"exited","exit_code":1,"last_error":null}""")
        assertEquals(ServiceFailure(1, null), serviceFailure(nonzero))
        val startup = JSONObject("""{"state":"dead","last_error":"Unsupported package runner"}""")
        assertEquals(ServiceFailure(null, "Unsupported package runner"), serviceFailure(startup))
        assertNull(serviceFailure(nonzero.put("state", "running")))
    }

    @Test fun cleanExitAndExpectedStopAreOrdinaryStoppedStates() {
        assertNull(serviceFailure(JSONObject("""{"state":"exited","exit_code":0}""")))
        assertNull(serviceFailure(JSONObject("""{"state":"exited","last_error":"Service execution timed out"}""")))
        assertNull(serviceFailure(JSONObject("""{"state":"dead","last_error":"Stopped by owner after observed worker death; guest exit code unknown"}""")))
    }

    @Test fun aNonzeroExitIsExpectedOnlyWhenOwnerAndWorkerConfirmStop() {
        val record = JSONObject().put("state", "exited").put("exit_code", 127)
        assertEquals(CONFIRMED_OWNER_STOP, workerExitReason(127, null, true, true))
        assertNull(serviceFailure(record.put("last_error", CONFIRMED_OWNER_STOP)))
        assertEquals(127, record.getInt("exit_code"))
        assertNull(workerExitReason(127, null, true, false))
        assertNull(workerExitReason(127, null, false, true))
        assertEquals(ServiceFailure(127, null), serviceFailure(record.put("last_error", JSONObject.NULL)))
        assertEquals("Worker control failed", workerExitReason(127, "Worker control failed", true, true))
    }

    @Test fun outputPreviewStripsTerminalControlsAndBoundsRecentText() {
        assertEquals(
            "cannot bind\nNot supported",
            serviceErrorPreview("\u001b[31mcannot bind\u001b[0m\n\u001b]0;window title\u0007Not supported\u202e"),
        )
        assertEquals(480, serviceErrorPreview("x".repeat(10_000))?.length)
        assertEquals("two\nthree\nfour\nfive", serviceErrorPreview("one\ntwo\nthree\nfour\nfive\n"))
    }
}
