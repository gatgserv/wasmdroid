package dev.wasmdroid.app.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.produceState
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import dev.wasmdroid.app.R
import dev.wasmdroid.app.engine.CONFIRMED_OWNER_STOP
import kotlinx.coroutines.CancellationException
import org.json.JSONObject

internal data class ServiceFailure(val exitCode: Int?, val reason: String?)

private val expectedStopReasons = setOf(
    "Stopped by owner after observed worker death; guest exit code unknown",
    "Service execution timed out",
    CONFIRMED_OWNER_STOP,
)
private val terminalControls = Regex("""\x1B(?:\[[0-?]*[ -/]*[@-~]|\][^\x07\x1B]*(?:\x07|\x1B\\))|[\p{Cc}\p{Cf}&&[^\r\n\t]]""")

internal fun serviceFailure(record: JSONObject): ServiceFailure? {
    val state = record.optString("state")
    if (state != "exited" && state != "dead") return null
    val reason = (record.opt("last_error") as? String)?.trim()?.takeIf { it.isNotEmpty() }
    if (reason != null && reason in expectedStopReasons) return null
    val exitCode = (record.opt("exit_code") as? Int)?.takeIf { it != 0 }
    return if (state == "dead" || exitCode != null || reason != null) {
        ServiceFailure(exitCode, reason?.let(::serviceErrorPreview))
    } else null
}

internal fun serviceErrorPreview(output: String): String? =
    terminalControls.replace(output.takeLast(4096), "")
        .lineSequence().map(String::trim).filter(String::isNotEmpty).toList()
        .takeLast(4).joinToString("\n").take(480).takeIf(String::isNotEmpty)

@Composable
internal fun ServiceFailureMessage(
    id: String,
    revision: Long,
    failure: ServiceFailure,
    connected: Boolean,
    model: AppModel,
) {
    Text(
        text = failure.exitCode?.let { stringResource(R.string.app_failed_exit_code, it) }
            ?: stringResource(R.string.app_failed),
        color = MaterialTheme.colorScheme.error,
    )
    val details = produceState(failure.reason, id, revision, failure, connected) {
        value = failure.reason
        if (failure.reason == null && connected) {
            try {
                val output = model.output(id)?.let(::JSONObject)
                value = (output?.opt("stderr") as? String)?.let(::serviceErrorPreview)
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (_: Exception) {
                // The terminal state and exit code remain available from storage.
            }
        }
    }.value
    details?.let {
        Text(
            text = it,
            style = MaterialTheme.typography.bodySmall,
            maxLines = 5,
            overflow = TextOverflow.Ellipsis,
        )
    }
}
