package dev.wasmdroid.app.ui

import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import dev.wasmdroid.app.R
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive

/** Embedded in app details/data, including applications without data volumes.
 * Confirmation/token state is private memory, never rememberSaveable or a Bundle. */
@Composable internal fun BrowserDataPane(instanceId: String, displayName: String, connected: Boolean, model: AppModel) {
    val scope = rememberCoroutineScope()
    val connection = rememberUpdatedState(connected)
    val controller = remember(instanceId, model) {
        BrowserDataController(scope, object : BrowserDataBackend {
            override val current: Boolean get() = connection.value
            override suspend fun state() = model.browserProfileState(instanceId)
            override suspend fun erase(revision: Long, profileToken: String) = model.eraseBrowserProfile(instanceId, revision, profileToken)
        })
    }
    DisposableEffect(controller) { onDispose { controller.close() } }
    LaunchedEffect(controller, connected) {
        if (!connected) { controller.disconnected(); return@LaunchedEffect }
        while (isActive) { controller.refresh(); delay(1_000) }
    }
    val state by controller.state.collectAsState()
    BrowserDataPane(displayName, connected, state, controller::requestConfirmation, controller::confirm,
        controller::dismissConfirmation, controller::reload)
}

@Composable internal fun BrowserDataPane(
    displayName: String, connected: Boolean, state: BrowserDataState,
    request: () -> Unit, confirm: (BrowserEraseConfirmation) -> Unit, dismiss: () -> Unit, reload: () -> Unit,
) {
    val snapshot = state.snapshot
    val unavailable = !connected || snapshot == null || state.problem == BrowserDataProblem.Unavailable
    val changed = state.problem == BrowserDataProblem.Changed
    val clearing = state.clearing || snapshot?.status == BrowserDataStatus.Clearing
    val unsupported = snapshot?.status == BrowserDataStatus.Unsupported || state.problem == BrowserDataProblem.Unsupported
    val inUse = snapshot?.status == BrowserDataStatus.InUse || state.problem == BrowserDataProblem.InUse
    val interrupted = snapshot?.status == BrowserDataStatus.Interrupted
    val failed = state.problem == BrowserDataProblem.Failed
    val loading = connected && state.reading && snapshot == null
    val label = when {
        clearing -> R.string.browser_data_clearing
        loading -> R.string.browser_data_loading
        unavailable -> R.string.browser_data_unavailable
        changed -> R.string.browser_data_changed
        unsupported -> R.string.browser_data_unsupported
        inUse -> R.string.browser_data_in_use
        interrupted -> R.string.browser_data_interrupted
        failed -> R.string.browser_data_failed
        else -> R.string.browser_data_available
    }
    OutlinedCard(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Text(stringResource(R.string.browser_data_title), style = MaterialTheme.typography.titleMedium)
            Text(stringResource(R.string.browser_data_preserved), style = MaterialTheme.typography.bodyMedium)
            Text(stringResource(label), style = MaterialTheme.typography.labelLarge,
                modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite })
            Text(stringResource(when {
                clearing -> R.string.browser_data_clearing_detail
                loading -> R.string.browser_data_loading_detail
                unavailable -> R.string.browser_data_unavailable_detail
                changed -> R.string.browser_data_changed_detail
                unsupported -> R.string.browser_data_unsupported_detail
                inUse -> R.string.browser_data_in_use_detail
                interrupted -> R.string.browser_data_interrupted_detail
                failed -> R.string.browser_data_failed_detail
                else -> R.string.browser_data_available_detail
            }))
            if (loading || clearing) LinearProgressIndicator(Modifier.fillMaxWidth())
            if (state.acknowledged && !unavailable && !clearing && !changed) {
                Text(stringResource(R.string.browser_data_cleared), modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite })
            }
            if (changed || (unavailable && connected)) {
                Button(enabled = connected && !state.reading && !clearing, onClick = reload) {
                    Text(stringResource(R.string.browser_data_reload))
                }
            } else {
                val clearLabel = stringResource(if (interrupted || failed) R.string.browser_data_retry else R.string.browser_data_clear)
                val accessibility = stringResource(R.string.browser_data_clear_named, displayName)
                Button(enabled = connected && !state.reading && !clearing && !unsupported && !inUse && snapshot?.canErase == true,
                    onClick = request, modifier = Modifier.semantics { contentDescription = accessibility }) {
                    Text(clearLabel)
                }
            }
            Text(stringResource(R.string.browser_data_scope), style = MaterialTheme.typography.bodySmall)
            Text(stringResource(R.string.browser_data_origin_note), style = MaterialTheme.typography.bodySmall)
        }
    }
    val confirmation = state.confirmation
    if (confirmation != null && connected && !state.clearing && !changed && !unsupported && !inUse) {
        AlertDialog(onDismissRequest = dismiss,
            title = { Text(stringResource(R.string.browser_data_confirm_title)) },
            text = { Text(stringResource(R.string.browser_data_confirm_body, displayName)) },
            confirmButton = { Button(enabled = !state.reading && snapshot?.canErase == true, onClick = { confirm(confirmation) }) {
                Text(stringResource(R.string.browser_data_confirm_clear))
            } },
            dismissButton = { TextButton(onClick = dismiss) { Text(stringResource(R.string.browser_data_keep)) } })
    }
}
