package com.picoo.camera.ui.theme

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp
import com.picoo.camera.R

/** REQ-PICOO-UI-003 — shared Picoo Camera visual tokens. */
object PicooColors {
    val Ink = Color(0xFF0B0D11)
    val Panel = Color(0xFF14171F)
    val Panel2 = Color(0xFF1B202C)
    val Panel3 = Color(0xFF242B3B)
    val Line = Color(0x14FFFFFF)
    val LineSubtle = Color(0x0AFFFFFF)
    val LineBold = Color(0x29FFFFFF)
    val Text = Color(0xFFF4F2ED)
    val Muted = Color(0xFF959DAE)
    val MutedDark = Color(0xFF656D7D)
    // Desktop HTML dark-theme brand tokens: primary #193CB8, highlight #2B7FFF.
    val Accent = Color(0xFF193CB8)
    val AccentHover = Color(0xFF155DFC)
    val Accent2 = Color(0xFF2B7FFF)
    val Ready = Color(0xFF3ECF8E)
    val Warn = Color(0xFFF0C14A)
    val Danger = Color(0xFFFF5C6C)
    val DangerBg = Color(0x24FF5C6C)
    val DangerText = Color(0xFFFFB0B8)
    val LiveBg = Color(0xFF040507)
}

object PicooFont {
    val Display = FontFamily.SansSerif
    val Body = FontFamily.SansSerif
    val Mono = FontFamily(Font(R.font.jetbrains_mono_regular, FontWeight.Normal))
}

private val PicooTypography = Typography(
    headlineLarge = TextStyle(
        fontFamily = PicooFont.Display,
        fontWeight = FontWeight.Bold,
        fontSize = 26.sp,
        lineHeight = 32.sp,
        letterSpacing = (-0.5).sp,
        color = PicooColors.Text,
    ),
    titleLarge = TextStyle(
        fontFamily = PicooFont.Display,
        fontWeight = FontWeight.Bold,
        fontSize = 24.sp,
        letterSpacing = (-0.3).sp,
        color = PicooColors.Text,
    ),
    titleMedium = TextStyle(
        fontFamily = PicooFont.Display,
        fontWeight = FontWeight.Bold,
        fontSize = 20.sp,
        letterSpacing = (-0.2).sp,
        color = PicooColors.Text,
    ),
    bodyLarge = TextStyle(
        fontFamily = PicooFont.Body,
        fontWeight = FontWeight.Normal,
        fontSize = 15.sp,
        lineHeight = 22.sp,
        color = PicooColors.Muted,
    ),
    bodyMedium = TextStyle(
        fontFamily = PicooFont.Body,
        fontSize = 14.sp,
        lineHeight = 20.sp,
        color = PicooColors.Muted,
    ),
    labelSmall = TextStyle(
        fontFamily = PicooFont.Body,
        fontWeight = FontWeight.SemiBold,
        fontSize = 11.sp,
        letterSpacing = 0.5.sp,
    ),
)

private val PicooDarkScheme = darkColorScheme(
    primary = PicooColors.Accent,
    onPrimary = Color(0xFFEFF6FF),
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
        typography = PicooTypography,
        content = content,
    )
}
