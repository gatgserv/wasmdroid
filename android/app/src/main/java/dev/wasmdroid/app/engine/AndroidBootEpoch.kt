package dev.wasmdroid.app.engine

import android.content.Context
import android.provider.Settings

/** Public API 24+ boot counter. Missing/denied/invalid reads fail closed for
 * automatic recovery; no guessed zero, wall-clock or random fallback. */
object AndroidBootEpoch {
    fun read(context:Context):ULong? = try {
        Settings.Global.getInt(context.contentResolver,Settings.Global.BOOT_COUNT)
            .takeIf{it>=0}?.toULong()
    }catch(_:Settings.SettingNotFoundException){null}
    catch(_:SecurityException){null}
    catch(_:RuntimeException){null}
}
