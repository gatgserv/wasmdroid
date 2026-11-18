package dev.wasmdroid.app

import android.content.Context
import android.content.res.Configuration
import java.util.Locale

/** Override only language/layout direction. Density, font scale and night mode
 * continue following the device. Do not change the device or guest locale. */
internal fun englishContext(base:Context):Context = base.createConfigurationContext(
    Configuration().apply { setLocale(Locale.ENGLISH) }
)
