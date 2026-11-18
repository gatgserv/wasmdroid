package dev.wasmdroid.app.engine

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/** Credential-encrypted application data becomes available at BOOT_COMPLETED.
 * Do not react to package upgrades, locked boot, sticky service recreation or
 * other broadcasts as an implicit request to run user applications. */
class EngineBootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Intent.ACTION_BOOT_COMPLETED) return
        try {
            context.startForegroundService(Intent(context, EngineService::class.java)
                .setAction(EngineService.ACTION_RESTORE_BOOT))
        } catch (failure: RuntimeException) {
            // Admission can be denied by OS/OEM background policy. Persist only
            // a bounded non-sensitive receipt; never claim guests were started.
            BootAdmissionReceipt.record(context, "foreground_start_denied")
        }
    }
}
