package com.mewo.reader.data

import android.content.Context
import android.content.res.Configuration
import com.mewo.reader.ui.theme.DisplayTheme
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

class ThemeStore(context: Context) {
    private val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
    private val _theme = MutableStateFlow(read(context))
    val theme: StateFlow<DisplayTheme> = _theme.asStateFlow()

    fun setTheme(theme: DisplayTheme) {
        prefs.edit().putString(KEY, theme.id).apply()
        _theme.value = theme
    }

    private fun read(context: Context): DisplayTheme {
        val stored = prefs.getString(KEY, null)
        if (stored != null) return DisplayTheme.fromId(stored)
        val night = context.resources.configuration.uiMode and
            Configuration.UI_MODE_NIGHT_MASK
        return if (night == Configuration.UI_MODE_NIGHT_YES) {
            DisplayTheme.XDark
        } else {
            DisplayTheme.XLight
        }
    }

    companion object {
        private const val PREFS = "mewo"
        private const val KEY = "display_theme"
    }
}
