package dev.wasmdroid.app

import android.content.Intent
import dev.wasmdroid.app.reports.*
import org.junit.Assert.*
import org.junit.Test

/** Actual Android Intent construction only; no browser is launched and no
 * issue is submitted by these definitions. UI/OS acceptance remains separate. */
class DiagnosticSharingTest {
    @Test fun issueFormContainsExactlyTheReviewedTextAndFixedOwnerDestination() {
        val preview=DiagnosticReport.preview(ReportIssue.fromCodes("network","application","downloading"),
            ReportPlatform.from(35,listOf("arm64-v8a")))
        val intent=diagnosticIssueIntent(preview)
        assertEquals(Intent.ACTION_VIEW,intent.action)
        val uri=intent.data!!
        assertEquals("https",uri.scheme);assertEquals("github.com",uri.host)
        assertNull(uri.userInfo);assertEquals(-1,uri.port);assertNull(uri.fragment)
        assertEquals("/gatgserv/wasmdroid/issues/new",uri.path)
        assertEquals(setOf("title","body"),uri.queryParameterNames)
        assertEquals(preview.text,uri.getQueryParameter("body"))
        assertEquals("WasmDroid diagnostic report",uri.getQueryParameter("title"))
        assertEquals(setOf(Intent.CATEGORY_BROWSABLE),intent.categories)
        assertNull(intent.type);assertNull(intent.extras)
        assertNull(intent.clipData);assertNull(intent.component);assertNull(intent.`package`)
        assertEquals(0,intent.flags)
    }
    @Test fun explicitUiOperationsCannotIntroducePrivateIdentifiersIntoTheReport() {
        for(operation in listOf("start","stop","restart","configure","data","search","catalog_refresh","remove","create","share")){
            val issue=ReportIssue.fromCodes("other",operation,"failed")
            assertEquals(operation,issue.operation.code)
            val text=DiagnosticReport.preview(issue,ReportPlatform.from(35,listOf("arm64-v8a"))).text
            assertTrue(text.contains("Operation: $operation\n"))
        }
        val text=DiagnosticReport.preview(ReportIssue.fromCodes("other","start:PRIVATE_APP_ID","failed"),
            ReportPlatform.from(35,listOf("arm64-v8a"))).text
        assertFalse(text.contains("PRIVATE_APP_ID"));assertTrue(text.contains("Operation: unknown\n"))
    }
}
