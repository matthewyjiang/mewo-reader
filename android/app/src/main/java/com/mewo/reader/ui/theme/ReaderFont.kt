package com.mewo.reader.ui.theme

import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp

/**
 * Reader post type scale. Chrome stays on the Material theme.
 *
 * Step 2 is DESIGN.md body: 17sp on 24sp (line 1.41). Neighbors step by about
 * 1.13, inside the 1.125-1.2 product-UI range. Floor 14sp is still body text.
 * Ceiling 28sp is 1.65x default, past the 18pt large-text line, without
 * collapsing a phone column to a handful of words.
 */
data class ReaderTypeScale(val step: Int) {
    init {
        require(step in MIN_STEP..MAX_STEP) {
            "Font size step $step is outside $MIN_STEP..$MAX_STEP"
        }
    }

    val bodySize = BODY_SP[step].sp
    val bodyLine = BODY_LINE_SP[step].sp
    val headingSize = HEADING_SP[step].sp
    val headingLine = HEADING_LINE_SP[step].sp
    val talkBackLabel = LABELS[step]

    fun bodyStyle(base: TextStyle): TextStyle =
        base.copy(fontSize = bodySize, lineHeight = bodyLine, fontWeight = FontWeight.Normal)

    fun headingStyle(base: TextStyle): TextStyle =
        base.copy(fontSize = headingSize, lineHeight = headingLine, fontWeight = FontWeight.Bold)

    companion object {
        const val MIN_STEP = 0
        const val MAX_STEP = 6
        const val DEFAULT_STEP = 2
        const val STEP_COUNT = 7

        private val BODY_SP = intArrayOf(14, 15, 17, 19, 21, 24, 28)
        private val BODY_LINE_SP = intArrayOf(20, 22, 24, 27, 30, 34, 39)
        private val HEADING_SP = intArrayOf(14, 16, 17, 19, 21, 24, 28)
        private val HEADING_LINE_SP = intArrayOf(18, 21, 22, 25, 27, 31, 36)
        private val LABELS = arrayOf(
            "Extra small",
            "Small",
            "Default",
            "Medium",
            "Large",
            "Extra large",
            "Largest",
        )

        val Default = ReaderTypeScale(DEFAULT_STEP)

        fun fromStored(step: Int): ReaderTypeScale =
            if (step in MIN_STEP..MAX_STEP) ReaderTypeScale(step) else Default
    }
}

data class ReaderFontController(
    val scale: ReaderTypeScale,
    val setStep: (Int) -> Unit,
)

val LocalReaderFont = staticCompositionLocalOf<ReaderFontController> {
    error("ReaderFontController missing")
}
