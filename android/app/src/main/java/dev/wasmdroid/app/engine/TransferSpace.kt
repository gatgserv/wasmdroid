package dev.wasmdroid.app.engine

import android.os.StatFs
import java.io.File

private const val TRANSFER_SPACE_RESERVE_BYTES = 16L * 1024 * 1024

/** Admission uses immediately writable space and leaves the system cache alone. */
internal fun File.availableTransferBytes(): Long {
    val available = try {
        StatFs(absolutePath).availableBytes
    } catch (_: IllegalArgumentException) {
        return 0
    }
    return (available - TRANSFER_SPACE_RESERVE_BYTES).coerceAtLeast(0)
}
