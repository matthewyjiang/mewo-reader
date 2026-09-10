package com.mewo.reader.ui.theme

import androidx.compose.material3.Typography
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp
import com.mewo.reader.R

val Atkinson = FontFamily(
    Font(R.font.atkinson_hyperlegible_regular, FontWeight.Normal),
    Font(R.font.atkinson_hyperlegible_bold, FontWeight.Bold),
)

fun mewoTypography(ink: androidx.compose.ui.graphics.Color): Typography {
    val base = TextStyle(
        fontFamily = Atkinson,
        color = ink,
    )
    return Typography(
        displayLarge = base.copy(fontSize = 32.sp, lineHeight = 36.sp, fontWeight = FontWeight.Bold),
        headlineLarge = base.copy(fontSize = 22.sp, lineHeight = 26.sp, fontWeight = FontWeight.Bold),
        titleLarge = base.copy(fontSize = 20.sp, lineHeight = 24.sp, fontWeight = FontWeight.Bold),
        titleMedium = base.copy(fontSize = 17.sp, lineHeight = 22.sp, fontWeight = FontWeight.Bold),
        titleSmall = base.copy(fontSize = 15.sp, lineHeight = 20.sp, fontWeight = FontWeight.Bold),
        bodyLarge = base.copy(fontSize = 17.sp, lineHeight = 24.sp, fontWeight = FontWeight.Normal),
        bodyMedium = base.copy(fontSize = 15.sp, lineHeight = 20.sp, fontWeight = FontWeight.Normal),
        labelLarge = base.copy(fontSize = 14.sp, lineHeight = 18.sp, fontWeight = FontWeight.Bold),
        labelMedium = base.copy(fontSize = 13.sp, lineHeight = 16.sp, fontWeight = FontWeight.Normal),
        labelSmall = base.copy(fontSize = 13.sp, lineHeight = 16.sp, fontWeight = FontWeight.Normal),
    )
}
