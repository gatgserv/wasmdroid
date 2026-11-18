package dev.wasmdroid.app

import android.content.Context
import android.content.pm.PackageManager
import android.os.Build

internal object LocalNetworkPermission {
    const val NAME = "android.permission.ACCESS_LOCAL_NETWORK"

    fun granted(context: Context): Boolean =
        Build.VERSION.SDK_INT < 37 || context.checkSelfPermission(NAME) == PackageManager.PERMISSION_GRANTED
}
