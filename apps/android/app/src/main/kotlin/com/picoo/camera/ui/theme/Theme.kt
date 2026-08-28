package com.picoo.camera.ui.theme

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

/** REQ-PICOO-UI-003 — dark graphite + coral accent (HTML prototype). */
object PicooColors {
    val Ink = Color(0xFF0E1014)
    val Panel = Color(0xFF171A21)
    val Panel2 = Color(0xFF1F2430)
    val Line = Color(0x14FFFFFF)
    val Text = Color(0xFFF3F0EA)
    val Muted = Color(0xFF9AA0AE)
    val Accent = Color(0xFFFF6A3D)
    val Accent2 = Color(0xFFFFB347)
    val Ready = Color(0xFF3ECF8E)
    val Warn = Color(0xFFF0C14A)
    val Danger = Color(0xFFFF5C6C)
    val DangerText = Color(0xFFFFB0B8)
}

private val PicooDarkScheme = darkColorScheme(
    primary = PicooColors.Accent,
    onPrimary = Color(0xFF1A0D08),
    secondary = PicooColors.Accent2,
    background = PicooColors.Ink,
    surface = PicooColors.Panel,
    surfaceVariant = PicooColors.Panel2,
    onBackground = PicooColors.Text,
    onSurface = PicooColors.Text,
    onSurfaceVariant = PicooColors.Muted,
    outline = PicooColors.Line,
    error = PicooColors.Danger,
)

@Composable
fun PicooCameraTheme(
    darkTheme: Boolean = true,
    content: @Composable () -> Unit,
) {
    MaterialTheme(
        colorScheme = PicooDarkScheme,
        content = content,
    )
}
