package dev.wasmdroid.app.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.res.stringResource
import dev.wasmdroid.app.R

// Exact public releases with successful runs documented in docs/verification.
private val androidTestedReferences = setOf(
    "wasmer.io/python/python@3.13.20",
    "wasmer.io/wasmer/static-web-server@2.44.0",
)

internal fun isAndroidTested(reference: String): Boolean = reference in androidTestedReferences

internal fun testedRegistryMatch(query: String): String? {
    val term = query.trim().replace(' ', '-').removePrefix("wasmer.io/")
    return androidTestedReferences.firstOrNull { reference ->
        val release = reference.removePrefix("wasmer.io/")
        val packageName = release.substringBefore('@')
        term.equals(packageName.substringAfter('/'), ignoreCase = true) ||
            term.equals(packageName, ignoreCase = true) ||
            term.equals(release, ignoreCase = true)
    }
}

@Composable
internal fun AndroidTestedLabel(reference: String) {
    if (isAndroidTested(reference)) {
        Text(
            text = stringResource(R.string.registry_tested_on_android),
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.primary,
        )
    }
}
