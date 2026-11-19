package dev.wasmdroid.app.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import dev.wasmdroid.app.R
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import org.json.JSONObject

internal data class ActiveLanAddresses(val ipv4: String?, val ipv6: String?) {
    companion object {
        fun fromFrame(frame: JSONObject): ActiveLanAddresses? {
            if (frame.optString("phase") != "active" || frame.optLong("grant_epoch") <= 0) return null
            val ipv4 = (frame.opt("url") as? String)?.takeIf { it.isNotBlank() }
            val ipv6 = (frame.opt("ipv6_url") as? String)?.takeIf { it.isNotBlank() }
            return if (ipv4 == null && ipv6 == null) null else ActiveLanAddresses(ipv4, ipv6)
        }
    }
}

@Composable
internal fun LanCardAddresses(observationKey: String, readStatus: suspend () -> String) {
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    val observe by rememberUpdatedState(readStatus)
    var addresses by remember(observationKey, lifecycle) { mutableStateOf<ActiveLanAddresses?>(null) }
    LaunchedEffect(observationKey, lifecycle) {
        lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            try {
                while (true) {
                    addresses = try {
                        ActiveLanAddresses.fromFrame(JSONObject(observe()))
                    } catch (failure: Exception) {
                        if (failure is CancellationException) throw failure
                        null
                    }
                    delay(1_000)
                }
            } finally {
                addresses = null
            }
        }
    }
    val current = addresses ?: return
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(stringResource(R.string.lan_addresses), style = MaterialTheme.typography.labelLarge)
        SelectionContainer {
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                current.ipv4?.let { Text(it, style = MaterialTheme.typography.bodySmall) }
                current.ipv6?.let { Text(it, style = MaterialTheme.typography.bodySmall) }
            }
        }
    }
}
