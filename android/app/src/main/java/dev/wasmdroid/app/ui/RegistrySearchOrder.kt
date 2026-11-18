package dev.wasmdroid.app.ui

import java.time.Instant
import java.time.OffsetDateTime
import java.time.format.DateTimeParseException

internal fun parseRegistryReleaseDate(value: Any?): Instant? {
    if (value !is String || value.getOrNull(4) != '-') return null
    return try {
        // Android 9's ISO instant parser accepts UTC but rejects numeric offsets.
        OffsetDateTime.parse(value).toInstant()
    } catch (_: DateTimeParseException) {
        null
    }
}

internal fun orderRegistryResults(
    rows: List<RegistrySearchRow>,
    order: RegistrySearchOrder,
): List<RegistrySearchRow> {
    val testedFirst = compareByDescending<RegistrySearchRow> {
        it.legacyWebcVersion == null && isAndroidTested(it.reference)
    }
    val comparator = when (order) {
        RegistrySearchOrder.Registry -> testedFirst
        RegistrySearchOrder.NewestRelease -> testedFirst.thenByDescending { it.releasedAt }
    }
    // Equal dates keep their original Wasmer order across the loaded pages.
    return rows.sortedWith(comparator)
}
