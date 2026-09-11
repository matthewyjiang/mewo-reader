package com.mewo.reader.data

import android.content.Context
import com.mewo.reader.ui.theme.ReaderTypeScale
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

class ReaderFontStore(context: Context) {
    private val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
    private val _scale = MutableStateFlow(read())
    val scale: StateFlow<ReaderTypeScale> = _scale.asStateFlow()

    fun setStep(step: Int) {
        require(step in ReaderTypeScale.MIN_STEP..ReaderTypeScale.MAX_STEP) {
            "Font size step $step is outside ${ReaderTypeScale.MIN_STEP}..${ReaderTypeScale.MAX_STEP}"
        }
        val next = ReaderTypeScale(step)
        prefs.edit().putInt(KEY, step).apply()
        _scale.value = next
    }

    private fun read(): ReaderTypeScale {
        if (!prefs.contains(KEY)) return ReaderTypeScale.Default
        return ReaderTypeScale.fromStored(prefs.getInt(KEY, ReaderTypeScale.DEFAULT_STEP))
    }

    companion object {
        private const val PREFS = "mewo"
        private const val KEY = "reader_font_step"
    }
}
