package dev.wasmdroid.app.ui

import android.content.Intent
import android.net.Uri
import android.provider.Settings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import dev.wasmdroid.app.LocalNetworkPermission
import dev.wasmdroid.app.R

internal class LocalNetworkAccess(
    val granted: Boolean,
    val requesting: Boolean,
    val requested: Boolean,
    val refresh: () -> Boolean,
    val request: () -> Unit,
    val settings: () -> Unit,
)

@Composable
internal fun rememberLocalNetworkAccess(): LocalNetworkAccess {
    val context = LocalContext.current
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    var granted by remember(context) { mutableStateOf(LocalNetworkPermission.granted(context)) }
    var requesting by rememberSaveable { mutableStateOf(false) }
    var requested by rememberSaveable { mutableStateOf(false) }
    fun refresh(): Boolean = LocalNetworkPermission.granted(context).also { granted = it }

    val permission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {
        requesting = false
        requested = true
        refresh()
    }
    val settings = rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) { refresh() }
    DisposableEffect(context, lifecycle) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) refresh()
        }
        lifecycle.addObserver(observer)
        onDispose { lifecycle.removeObserver(observer) }
    }

    return LocalNetworkAccess(granted, requesting, requested, ::refresh, request = {
        if (!requesting && !refresh()) {
            requesting = true
            permission.launch(LocalNetworkPermission.NAME)
        }
    }, settings = {
        settings.launch(
            Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS)
                .setData(Uri.fromParts("package", context.packageName, null)),
        )
    })
}

@Composable
internal fun LocalNetworkAccessActions(access: LocalNetworkAccess, message: Int) {
    Text(stringResource(message))
    TextButton(enabled = !access.requesting, onClick = access.request) {
        Text(stringResource(R.string.android_local_network_allow))
    }
    if (access.requested) {
        TextButton(enabled = !access.requesting, onClick = access.settings) {
            Text(stringResource(R.string.android_local_network_settings))
        }
    }
}
