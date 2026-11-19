package dev.wasmdroid.app.reports

import android.os.Build
import dev.wasmdroid.app.BuildConfig

/** Fixed structured producer codes. They describe an observation, not a proven
 * cause. No exception message, identifier or arbitrary text is retained. */
internal enum class ReportFailure(val code:String) {
    Space("space"), Busy("busy"), Integrity("integrity"), Unsupported("unsupported"),
    Network("network"), LocalInput("local_input"), Cancelled("cancelled"), Other("other");

    companion object {
        fun fromCode(value:String?):ReportFailure =
            if (value == "registry_webc_version") Unsupported else entries.firstOrNull { it.code == value } ?: Other
    }
}

internal enum class ReportOperation(val code:String) {
    Application("application"), Image("image"), CatalogImage("catalog_image"),
    LocalImport("local_import"), Start("start"), Stop("stop"), Restart("restart"),
    Configure("configure"), Data("data"), Search("search"), CatalogRefresh("catalog_refresh"),
    Remove("remove"), Create("create"), Share("share"), Unknown("unknown");

    companion object {
        fun fromCode(value:String?):ReportOperation = entries.firstOrNull { it.code == value } ?: Unknown
    }
}

/** Exact current package status/progress tokens. The caller selects the actual
 * structured observation; this model never guesses a stage from an exception. */
internal enum class ReportStage(val code:String) {
    Active("active"), AwaitingReview("awaiting_review"), Publishing("publishing"),
    Committed("committed"), PreparingApplication("preparing_app"), Installed("installed"),
    Cancelled("cancelled"), Failed("failed"), Resolving("resolving"), Downloading("downloading"),
    Verifying("verifying"), ReadingLocal("reading_local"), InspectingLocal("inspecting_local"),
    InstallingLocal("installing_local"), Unknown("unknown");

    companion object {
        fun fromCode(value:String?):ReportStage = entries.firstOrNull { it.code == value } ?: Unknown
    }
}

internal data class ReportIssue(
    val failure:ReportFailure,
    val operation:ReportOperation,
    val stage:ReportStage,
) {
    companion object {
        /** Map immediately to enums; the raw input strings are never fields. */
        fun fromCodes(errorKind:String?, operationKind:String?, stage:String?):ReportIssue = ReportIssue(
            ReportFailure.fromCode(errorKind), ReportOperation.fromCode(operationKind), ReportStage.fromCode(stage))
    }
}

/** Closed platform fact set. Unsupported ABI strings and out-of-range integers
 * become fixed unknown markers. No model/manufacturer, serial or OS build ID. */
internal class ReportPlatform private constructor(internal val sdk:String, internal val abis:String) {
    override fun toString():String = "ReportPlatform(<allowlisted platform facts>)"

    companion object {
        private val abiCodes = listOf("arm64-v8a", "armeabi-v7a", "x86_64", "x86")

        /** Pure normalization for deterministic source-model checks. Only the
         * first eight bounded ABI observations are considered; order is fixed. */
        fun from(sdkInt:Int, supportedAbis:List<String>):ReportPlatform {
            val selected = supportedAbis.take(8).mapNotNull { value -> abiCodes.firstOrNull { it == value } }.toSet()
            val abis = abiCodes.filter { it in selected }.joinToString(", ").ifEmpty { "unknown" }
            return ReportPlatform(sdkInt.takeIf { it in 1..1000 }?.toString() ?: "unknown", abis)
        }

        /** Android adapter reads exactly these two public facts. Host-only
         * preview checks do not execute or validate this Android adapter. */
        fun current():ReportPlatform = from(Build.VERSION.SDK_INT, Build.SUPPORTED_ABIS.asList())
    }
}

internal interface DiagnosticPreview {
    val text:String
    val utf8Bytes:Int
}

/** A pure bounded preview, with no IO, destination, persistence or send action.
 * It is not a redactor for arbitrary logs: those inputs have no API here. */
internal object DiagnosticReport {
    const val MAX_UTF8_BYTES = 1024
    private val versionCharacters = Regex("[A-Za-z0-9][A-Za-z0-9._+\\-]{0,63}")
    private class Preview(override val text:String):DiagnosticPreview {
        override val utf8Bytes:Int get() = text.toByteArray(Charsets.UTF_8).size
        override fun toString():String = "DiagnosticPreview(<structured public fields>)"
    }

    fun preview(issue:ReportIssue, platform:ReportPlatform):DiagnosticPreview {
        // Only this application's generated build constants supply version.
        // There is intentionally no caller-supplied version or app ID argument.
        val version = BuildConfig.VERSION_NAME.takeIf { versionCharacters.matches(it) } ?: "unknown"
        val versionCode = BuildConfig.VERSION_CODE.takeIf { it >= 0 }?.toString() ?: "unknown"
        val text = """
            WasmDroid structured diagnostic
            Format: 1
            Error code: ${issue.failure.code}
            Operation: ${issue.operation.code}
            Observed stage: ${issue.stage.code}
            App version: $version ($versionCode)
            Android SDK: ${platform.sdk}
            Supported ABIs: ${platform.abis}

            The code and observed stage do not establish the underlying cause.
        """.trimIndent() + "\n"
        check(text.toByteArray(Charsets.UTF_8).size <= MAX_UTF8_BYTES) { "Structured report exceeds preview bound" }
        return Preview(text)
    }
}
