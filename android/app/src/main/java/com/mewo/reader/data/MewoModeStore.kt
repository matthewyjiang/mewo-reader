package com.mewo.reader.data

import android.content.Context
import com.mewo.reader.LauncherIconSwitcher
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

class MewoModeStore(context: Context) {
    private val appContext = context.applicationContext
    // The persisted launcher component state is the source of truth. A separate
    // backed-up preference could disagree with it after a restore or process death.
    private val _enabled = MutableStateFlow(LauncherIconSwitcher.isEnabled(appContext))
    val enabled: StateFlow<Boolean> = _enabled.asStateFlow()

    /** Publish only after the persistent launcher switch succeeds. */
    fun setEnabled(enabled: Boolean) {
        LauncherIconSwitcher.setEnabled(appContext, enabled)
        _enabled.value = enabled
    }
}
