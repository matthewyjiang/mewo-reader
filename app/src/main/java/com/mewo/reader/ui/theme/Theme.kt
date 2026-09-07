package com.mewo.reader.ui.theme

import android.app.Activity
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.SideEffect
import androidx.compose.ui.platform.LocalView
import androidx.core.view.WindowCompat

private val DarkScheme = darkColorScheme(
    primary = XBlue,
    onPrimary = XPaper,
    secondary = XMute,
    onSecondary = XInk,
    background = XBlack,
    onBackground = XInk,
    surface = XBlack,
    onSurface = XInk,
    surfaceVariant = XHover,
    onSurfaceVariant = XMute,
    outline = XLine,
    outlineVariant = XLine,
    error = XPink,
    onError = XPaper,
)

private val LightScheme = lightColorScheme(
    primary = XBlue,
    onPrimary = XPaper,
    secondary = XMuteLight,
    onSecondary = XInkLight,
    background = XPaper,
    onBackground = XInkLight,
    surface = XPaper,
    onSurface = XInkLight,
    surfaceVariant = XHoverLight,
    onSurfaceVariant = XMuteLight,
    outline = XLineLight,
    outlineVariant = XLineLight,
    error = XPink,
    onError = XPaper,
)

@Composable
fun MewoTheme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    content: @Composable () -> Unit,
) {
    val scheme = if (darkTheme) DarkScheme else LightScheme
    val view = LocalView.current
    if (!view.isInEditMode) {
        SideEffect {
            val window = (view.context as Activity).window
            WindowCompat.getInsetsController(window, view).apply {
                isAppearanceLightStatusBars = !darkTheme
                isAppearanceLightNavigationBars = !darkTheme
            }
        }
    }
    MaterialTheme(
        colorScheme = scheme,
        typography = mewoTypography(scheme.onBackground),
        content = content,
    )
}
