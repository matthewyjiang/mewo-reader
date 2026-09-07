package com.mewo.reader.ui.theme

import android.app.Activity
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.platform.LocalView
import androidx.core.view.WindowCompat

val LocalMewoColors = staticCompositionLocalOf { DisplayTheme.XDark.palette }

data class ThemeController(
    val current: DisplayTheme,
    val setTheme: (DisplayTheme) -> Unit,
)

val LocalThemeController = staticCompositionLocalOf<ThemeController> {
    error("ThemeController missing")
}

fun MewoPalette.toColorScheme(): ColorScheme {
    return if (isDark) {
        darkColorScheme(
            primary = accent,
            onPrimary = onAccent,
            secondary = mute,
            onSecondary = ink,
            background = ground,
            onBackground = ink,
            surface = ground,
            onSurface = ink,
            surfaceVariant = hover,
            onSurfaceVariant = mute,
            outline = line,
            outlineVariant = line,
            error = like,
            onError = onAccent,
        )
    } else {
        lightColorScheme(
            primary = accent,
            onPrimary = onAccent,
            secondary = mute,
            onSecondary = ink,
            background = ground,
            onBackground = ink,
            surface = ground,
            onSurface = ink,
            surfaceVariant = hover,
            onSurfaceVariant = mute,
            outline = line,
            outlineVariant = line,
            error = like,
            onError = onAccent,
        )
    }
}

@Composable
fun MewoTheme(
    theme: DisplayTheme,
    content: @Composable () -> Unit,
) {
    val palette = theme.palette
    val scheme = palette.toColorScheme()
    val view = LocalView.current
    if (!view.isInEditMode) {
        SideEffect {
            val window = (view.context as Activity).window
            WindowCompat.getInsetsController(window, view).apply {
                isAppearanceLightStatusBars = !palette.isDark
                isAppearanceLightNavigationBars = !palette.isDark
            }
        }
    }
    CompositionLocalProvider(LocalMewoColors provides palette) {
        MaterialTheme(
            colorScheme = scheme,
            typography = mewoTypography(palette.ink),
            content = content,
        )
    }
}
