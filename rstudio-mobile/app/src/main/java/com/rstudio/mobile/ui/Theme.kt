package com.rstudio.mobile.ui

import android.app.Activity
import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.SideEffect
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.core.view.WindowCompat

private val DarkRStudio = androidx.compose.ui.graphics.Color(0xFF252526)
private val DarkAccent = androidx.compose.ui.graphics.Color(0xFF0E639C)
private val DarkHighlight = androidx.compose.ui.graphics.Color(0xFF007ACC)
private val DarkBackground = androidx.compose.ui.graphics.Color(0xFF1E1E1E)
private val DarkSurface = androidx.compose.ui.graphics.Color(0xFF2D2D30)
private val DarkTextPrimary = androidx.compose.ui.graphics.Color(0xFFD4D4D4)
private val DarkTextOnPrimary = androidx.compose.ui.graphics.Color(0xFFFFFFFF)
private val DarkTextOnSecondary = androidx.compose.ui.graphics.Color(0xFFFFFFFF)

private val LightRStudio = androidx.compose.ui.graphics.Color(0xFF0E639C)
private val LightAccent = androidx.compose.ui.graphics.Color(0xFF007ACC)
private val LightHighlight = androidx.compose.ui.graphics.Color(0xFF339933)
private val LightBackground = androidx.compose.ui.graphics.Color(0xFFF7F7F7)
private val LightSurface = androidx.compose.ui.graphics.Color(0xFFFFFFFF)
private val LightTextPrimary = androidx.compose.ui.graphics.Color(0xFF333333)
private val LightTextOnPrimary = androidx.compose.ui.graphics.Color(0xFFFFFFFF)
private val LightTextOnSecondary = androidx.compose.ui.graphics.Color(0xFFFFFFFF)

private val DarkColorScheme = darkColorScheme(
    primary = DarkRStudio,
    secondary = DarkAccent,
    tertiary = DarkHighlight,
    background = DarkBackground,
    surface = DarkSurface,
    onPrimary = DarkTextOnPrimary,
    onSecondary = DarkTextOnSecondary,
    onBackground = DarkTextPrimary,
    onSurface = DarkTextPrimary,
)

private val LightColorScheme = lightColorScheme(
    primary = LightRStudio,
    secondary = LightAccent,
    tertiary = LightHighlight,
    background = LightBackground,
    surface = LightSurface,
    onPrimary = LightTextOnPrimary,
    onSecondary = LightTextOnSecondary,
    onBackground = LightTextPrimary,
    onSurface = LightTextPrimary,
)

@Composable
fun RStudioTheme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    dynamicColor: Boolean = false,
    content: @Composable () -> Unit
) {
    val colorScheme = when {
        dynamicColor && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S -> {
            val context = LocalContext.current
            if (darkTheme) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
        }
        darkTheme -> DarkColorScheme
        else -> LightColorScheme
    }
    val view = LocalView.current
    if (!view.isInEditMode) {
        SideEffect {
            val window = (view.context as Activity).window
            WindowCompat.getInsetsController(window, view).isAppearanceLightStatusBars = !darkTheme
        }
    }

    MaterialTheme(
        colorScheme = colorScheme,
        typography = MaterialTheme.typography,
        shapes = MaterialTheme.shapes,
        content = content
    )
}
