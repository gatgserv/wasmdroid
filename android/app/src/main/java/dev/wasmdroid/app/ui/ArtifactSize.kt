package dev.wasmdroid.app.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.res.stringResource
import dev.wasmdroid.app.R
import org.json.JSONObject

/** Installed artifact metadata, not graph size or allocated filesystem blocks. */
@Composable
internal fun ArtifactSize(image: JSONObject) {
    val bytes = when (val value = image.opt("size_bytes")) {
        is Int -> value.takeIf { it >= 0 }?.toULong()
        is Long -> value.takeIf { it >= 0 }?.toULong()
        else -> null // Do not coerce a lossy floating-point value into a size.
    }
    val size = bytes?.let(::formatStorageBytes)
        ?: stringResource(R.string.artifact_size_unknown)
    Text(
        stringResource(R.string.artifact_file_size, size),
        style = MaterialTheme.typography.bodySmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
}
