package dev.wasmdroid.app.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import dev.wasmdroid.app.R
import org.json.JSONObject

@Composable
internal fun AutomaticStartDialog(controller: AutomaticStartController, app: AppState) {
    val selection by controller.state.collectAsState()
    val current = selection ?: return
    val source = current.source
    val record = remember(app.instances, current.id) {
        app.instances.asSequence().map(::JSONObject).firstOrNull { it.getString("id") == current.id }
    }
    val changed = source != null && (record?.getLong("version")?.toULong() != source.version || current.id in app.memberships)
    val occupied = source?.editable == false || current.id in app.controllers
    val available = app.connected && controller.available()
    val enabled = available && source != null && !changed && !occupied && !current.saving && !current.failed
    val problem = when {
        current.loading || current.saving -> null
        current.failed -> R.string.automation_edit_failed
        !available -> R.string.automation_edit_unavailable
        changed -> R.string.automation_edit_changed
        occupied -> R.string.automation_edit_stop
        else -> null
    }

    AlertDialog(
        onDismissRequest = { controller.dismiss(current.epoch) },
        title = { Text(stringResource(R.string.automation_title)) },
        text = {
            Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                source?.let { Text(it.name, style = MaterialTheme.typography.titleMedium) }
                if (current.loading || current.saving) LinearProgressIndicator(Modifier.fillMaxWidth())
                if (problem != null) {
                    Text(stringResource(problem), color = MaterialTheme.colorScheme.error)
                    TextButton(onClick = { controller.reload(current.epoch) }) { Text(stringResource(R.string.reload)) }
                }
                if (source != null) {
                    AutomaticStartControls(current.startOnBoot, current.restartPolicy, enabled,
                        { controller.boot(current.epoch, it) }, { controller.restart(current.epoch, it) })
                }
            }
        },
        confirmButton = {
            Button(enabled = enabled, onClick = { controller.save(current.epoch) }) {
                Text(stringResource(R.string.automation_edit_save))
            }
        },
        dismissButton = {
            TextButton(enabled = !current.saving, onClick = { controller.dismiss(current.epoch) }) { Text(stringResource(R.string.cancel)) }
        },
    )
}
