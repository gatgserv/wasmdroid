package dev.wasmdroid.app

import android.os.Build
import dev.wasmdroid.app.reports.*
import org.junit.Assert.*
import org.junit.Test

/** Structured model definitions only. Pure checks need the own BuildConfig on
 * their classpath; current() below is separate actual Android fact coverage.
 * No guest logs, report sharing, destination or network is exercised. */
class DiagnosticReportTest {
    private fun preview(error:String?, operation:String?, stage:String?, sdk:Int=35,
        abis:List<String> = listOf("arm64-v8a")):String =
        DiagnosticReport.preview(ReportIssue.fromCodes(error, operation, stage), ReportPlatform.from(sdk, abis)).text

    @Test fun actualStructuredFailureCodesAndOperationKindsHaveFixedOutput() {
        for (code in listOf("space","busy","integrity","unsupported","network","local_input","cancelled","other")) {
            assertTrue(preview(code, "image", "failed").contains("Error code: $code\n"))
        }
        for (kind in listOf("application","image","catalog_image","local_import")) {
            assertTrue(preview("other", kind, "failed").contains("Operation: $kind\n"))
        }
    }

    @Test fun arbitraryExceptionLikeTextNeverInfersAnErrorCategory() {
        for (raw in listOf("DNS network connection to private.example failed", "digest of /private/file changed",
            "no space left: secret-volume", "unsupported command --password=PRIVATE", "operation cancelled for PRIVATE")) {
            val report = preview(raw, "image", "failed")
            assertTrue(report.contains("Error code: other\n"))
            assertFalse(report.contains(raw))
            assertFalse(report.contains("PRIVATE"))
        }
    }

    @Test fun unknownInjectionValuesAreReplacedWithoutNormalizationOrEcho() {
        val privateValues = listOf("NETWORK", " network ", "network\nSECRET=private", "https://user:pass@private.invalid/a",
            "wasmer.io/private/package@1.0.0", "pull:private-app-id", "/data/private/path", "content://private/file")
        for (raw in privateValues) {
            val report = preview(raw, raw, raw)
            assertTrue(report.contains("Error code: other\nOperation: unknown\nObserved stage: unknown\n"))
            assertFalse(report.contains(raw))
        }
        assertEquals(preview(null, null, null), preview("arbitrary", "arbitrary", "arbitrary"))
    }

    @Test fun stagesOnlyDescribeExactKnownStructuredObservations() {
        val stages = listOf("active","awaiting_review","publishing","committed","preparing_app","installed",
            "cancelled","failed","resolving","downloading","verifying","reading_local","inspecting_local","installing_local")
        for (stage in stages) assertTrue(preview("other", "local_import", stage).contains("Observed stage: $stage\n"))
        assertTrue(preview("network", "image", "the connection failed while downloading").contains("Observed stage: unknown\n"))
        assertFalse(preview("integrity", "application", "preparing_app").contains("Cause: integrity"))
    }

    @Test fun platformValuesAreClosedAndCannotInjectIdentityOrExtraFields() {
        val report = preview("other", "image", "active", 35,
            listOf("private-device-serial", "arm64-v8a", "x86_64", "arm64-v8a", "/private/path", "x86\nSECRET"))
        assertTrue(report.contains("Android SDK: 35\nSupported ABIs: arm64-v8a, x86_64\n"))
        assertFalse(report.contains("private")); assertFalse(report.contains("SECRET"))
        for (sdk in listOf(Int.MIN_VALUE, -1, 0, 1001, Int.MAX_VALUE)) {
            assertTrue(preview("other", "image", "active", sdk, listOf("PRIVATE")).contains(
                "Android SDK: unknown\nSupported ABIs: unknown\n"))
        }
    }

    @Test fun reportOwnsItsNormalizedSnapshotAndDoesNotRetainCallerCollections() {
        val input = mutableListOf("arm64-v8a")
        val platform = ReportPlatform.from(35, input)
        val issue = ReportIssue.fromCodes("network", "image", "downloading")
        val expected = DiagnosticReport.preview(issue, platform).text
        input.clear(); input.add("PRIVATE_DEVICE")
        assertEquals(expected, DiagnosticReport.preview(issue, platform).text)
        assertFalse(platform.toString().contains("PRIVATE_DEVICE"))
    }

    @Test fun arbitrarilyLargeUnknownInputsCannotExpandOrTruncateTheFixedReport() {
        val input = "PRIVATE:/secret/".repeat(100_000)
        val rendered = DiagnosticReport.preview(ReportIssue.fromCodes(input, input, input),
            ReportPlatform.from(1000, List(1000) { input }))
        assertTrue(rendered.utf8Bytes <= DiagnosticReport.MAX_UTF8_BYTES)
        assertFalse(rendered.text.contains("PRIVATE"))
        assertTrue(rendered.text.contains("Supported ABIs: unknown\n"))
        assertTrue(rendered.text.endsWith("The code and observed stage do not establish the underlying cause.\n"))
    }

    @Test fun onlyOwnBuildVersionAndFixedPublicFieldsEnterThePreview() {
        val issue = ReportIssue.fromCodes("space", "image", "failed")
        val rendered = DiagnosticReport.preview(issue, ReportPlatform.from(35, listOf("arm64-v8a")))
        assertTrue(rendered.text.contains("App version: ${BuildConfig.VERSION_NAME} (${BuildConfig.VERSION_CODE})\n"))
        val keys = rendered.text.lines().filter { it.contains(':') }.map { it.substringBefore(':') }.toSet()
        assertEquals(setOf("Format","Error code","Operation","Observed stage","App version","Android SDK","Supported ABIs"), keys)
        assertFalse(rendered.toString().contains(BuildConfig.VERSION_NAME))
        assertFalse(DiagnosticReport::class.java.declaredMethods.filter { it.name == "preview" }
            .flatMap { it.parameterTypes.toList() }.any { it == String::class.java || Throwable::class.java.isAssignableFrom(it) })
    }

    @Test fun androidAdapterReadsOnlyCurrentSdkAndAllowlistedSupportedAbis() {
        // This case is not a host-only assertion: actual execution needs Android.
        val issue = ReportIssue.fromCodes("other", "image", "active")
        val actual = DiagnosticReport.preview(issue, ReportPlatform.current()).text
        val expected = DiagnosticReport.preview(issue, ReportPlatform.from(Build.VERSION.SDK_INT, Build.SUPPORTED_ABIS.asList())).text
        assertEquals(expected, actual)
    }
}
