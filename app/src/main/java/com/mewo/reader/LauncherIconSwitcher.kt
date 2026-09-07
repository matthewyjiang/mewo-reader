package com.mewo.reader

import android.content.ComponentName
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build

/** Switch launch aliases, enabling the destination before disabling the old one. */
object LauncherIconSwitcher {
    /** PackageManager persists this choice across process restarts and app updates. */
    fun isEnabled(context: Context): Boolean =
        context.packageManager.getComponentEnabledSetting(
            ComponentName(context.packageName, "${context.packageName}.TeddyLauncher"),
        ) == PackageManager.COMPONENT_ENABLED_STATE_ENABLED

    fun setEnabled(context: Context, enabled: Boolean) {
        val manager = context.packageManager
        val book = ComponentName(context.packageName, "${context.packageName}.BookLauncher")
        val teddy = ComponentName(context.packageName, "${context.packageName}.TeddyLauncher")
        val destination = if (enabled) teddy else book
        val previous = if (enabled) book else teddy
        val on = PackageManager.COMPONENT_ENABLED_STATE_ENABLED
        val off = PackageManager.COMPONENT_ENABLED_STATE_DISABLED
        val flags = PackageManager.DONT_KILL_APP
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            manager.setComponentEnabledSettings(
                listOf(
                    PackageManager.ComponentEnabledSetting(destination, on, flags),
                    PackageManager.ComponentEnabledSetting(previous, off, flags),
                ),
            )
        } else {
            val original = manager.getComponentEnabledSetting(destination)
            manager.setComponentEnabledSetting(destination, on, flags)
            try {
                manager.setComponentEnabledSetting(previous, off, flags)
            } catch (error: RuntimeException) {
                // Best-effort rollback; never disable the old entry first.
                runCatching { manager.setComponentEnabledSetting(destination, original, flags) }
                throw error
            }
        }
    }
}
