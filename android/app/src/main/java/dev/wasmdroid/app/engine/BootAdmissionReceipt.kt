package dev.wasmdroid.app.engine

import android.content.Context
import androidx.core.content.edit

/** Observability only. The source of boot authorization stays in InstanceSpec;
 * these preferences never contain launch identities or decide what may run. */
object BootAdmissionReceipt {
    private const val PREFERENCES = "boot-admission-result"
    private val outcomes = setOf("foreground_start_denied", "restore_failed", "restore_completed", "restore_cancelled")

    fun record(context: Context, outcome: String) {
        require(outcome in outcomes) { "Unknown boot admission outcome" }
        context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE).edit {
            putString("outcome", outcome)
            putLong("recorded_at_ms", System.currentTimeMillis())
        }
    }

    fun latest(context: Context): String? =
        context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE).getString("outcome", null)
}
