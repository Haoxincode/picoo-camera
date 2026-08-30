package com.picoo.camera.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.picoo.camera.R

/** ARCH-PICOO-UI-002 — primitive tokens are confined to this platform adapter. */
private object PicooPrimitiveLight {
    val Canvas = Color(0xFFFFFFFF)
    val Chrome = Color(0xFFFAFAFA)
    val Subtle = Color(0xFFF5F5F5)
    val Border = Color(0xFFE5E5E5)
    val Foreground = Color(0xFF0A0A0A)
    val Muted = Color(0xFF737373)
    val Primary = Color(0xFF1447E6)
    val Highlight = Color(0xFF155DFC)
    val Success = Color(0xFF008A5C)
    val Warning = Color(0xFF9A6500)
    val Danger = Color(0xFFE7000B)
}

private object PicooPrimitiveDark {
    val Canvas = Color(0xFF0A0A0A)
    val Chrome = Color(0xFF171717)
    val Subtle = Color(0xFF262626)
    val Border = Color(0x1AFFFFFF)
    val Foreground = Color(0xFFFAFAFA)
    val Muted = Color(0xFFA1A1A1)
    val Primary = Color(0xFF193CB8)
    val Highlight = Color(0xFF2B7FFF)
    val Success = Color(0xFF00BC7D)
    val Warning = Color(0xFFF0C14A)
    val Danger = Color(0xFFFF6467)
}

/** Semantic colors consumed by Picoo components and feature screens. */
@Immutable
data class PicooSemanticColors(
    val surfacePage: Color,
    val surfaceGroup: Color,
    val surfaceSubtle: Color,
    val contentPrimary: Color,
    val contentMuted: Color,
    val borderDefault: Color,
    val actionPrimary: Color,
    val actionHighlight: Color,
    val actionOnPrimary: Color,
    val statusSuccess: Color,
    val statusWarning: Color,
    val statusDanger: Color,
    val focusRing: Color,
)

private val PicooLightColors = PicooSemanticColors(
    surfacePage = PicooPrimitiveLight.Canvas,
    surfaceGroup = PicooPrimitiveLight.Chrome,
    surfaceSubtle = PicooPrimitiveLight.Subtle,
    contentPrimary = PicooPrimitiveLight.Foreground,
    contentMuted = PicooPrimitiveLight.Muted,
    borderDefault = PicooPrimitiveLight.Border,
    actionPrimary = PicooPrimitiveLight.Primary,
    actionHighlight = PicooPrimitiveLight.Highlight,
    actionOnPrimary = Color.White,
    statusSuccess = PicooPrimitiveLight.Success,
    statusWarning = PicooPrimitiveLight.Warning,
    statusDanger = PicooPrimitiveLight.Danger,
    focusRing = PicooPrimitiveLight.Highlight,
)

private val PicooDarkColors = PicooSemanticColors(
    surfacePage = PicooPrimitiveDark.Canvas,
    surfaceGroup = PicooPrimitiveDark.Chrome,
    surfaceSubtle = PicooPrimitiveDark.Subtle,
    contentPrimary = PicooPrimitiveDark.Foreground,
    contentMuted = PicooPrimitiveDark.Muted,
    borderDefault = PicooPrimitiveDark.Border,
    actionPrimary = PicooPrimitiveDark.Primary,
    actionHighlight = PicooPrimitiveDark.Highlight,
    actionOnPrimary = Color.White,
    statusSuccess = PicooPrimitiveDark.Success,
    statusWarning = PicooPrimitiveDark.Warning,
    statusDanger = PicooPrimitiveDark.Danger,
    focusRing = PicooPrimitiveDark.Highlight,
)

@Immutable
data class PicooDimensions(
    val space2: Dp = 2.dp,
    val space4: Dp = 4.dp,
    val space8: Dp = 8.dp,
    val space12: Dp = 12.dp,
    val space16: Dp = 16.dp,
    val space24: Dp = 24.dp,
    val space32: Dp = 32.dp,
    val radiusControl: Dp = 7.dp,
    val radiusSurface: Dp = 15.dp,
    val iconCompact: Dp = 16.dp,
    val iconStandard: Dp = 20.dp,
    val iconEmphasis: Dp = 24.dp,
    val iconHero: Dp = 32.dp,
    val touchTarget: Dp = 48.dp,
    val cameraStopTarget: Dp = 64.dp,
    val cameraStopStroke: Dp = 3.5.dp,
    val maxContentWidth: Dp = 680.dp,
)

@Immutable
data class PicooMotion(
    val fastMillis: Int = 220,
    val normalMillis: Int = 280,
    val deliberateMillis: Int = 500,
)

private val LocalPicooColors = staticCompositionLocalOf { PicooLightColors }
private val LocalPicooDimensions = staticCompositionLocalOf { PicooDimensions() }
private val LocalPicooMotion = staticCompositionLocalOf { PicooMotion() }

object PicooTheme {
    val colors: PicooSemanticColors
        @Composable get() = LocalPicooColors.current
    val dimensions: PicooDimensions
        @Composable get() = LocalPicooDimensions.current
    val motion: PicooMotion
        @Composable get() = LocalPicooMotion.current
}

/** Camera context is deliberately dark and independent from the system appearance. */
object PicooCameraColors {
    val Surface = Color(0xFF040507)
    val Overlay = Color(0xB30A0A0A)
    val Control = Color(0x29FFFFFF)
    val ControlBorder = Color(0x38FFFFFF)
    val Content = Color.White
    val ContentMuted = Color(0xA6FFFFFF)
    val Selected = Color(0xFF2B7FFF)
    val Success = Color(0xFF00BC7D)
    val Warning = Color(0xFFF0C14A)
    val Danger = Color(0xFFFF6467)
}

/**
 * Temporary Camera-context compatibility aliases. Control screens must consume [PicooTheme].
 * Removing this object is tracked by REQ-PICOO-UI-012 after StreamingScreen is fully tokenized.
 */
object PicooColors {
    val Ink = PicooCameraColors.Surface
    val Panel = Color(0xFF14171F)
    val Panel2 = Color(0xFF1B202C)
    val Panel3 = Color(0xFF242B3B)
    val Line = PicooCameraColors.ControlBorder
    val LineSubtle = Color(0x0AFFFFFF)
    val LineBold = Color(0x29FFFFFF)
    val Text = PicooCameraColors.Content
    val Muted = Color(0xFF959DAE)
    val MutedDark = Color(0xFF656D7D)
    val Accent = Color(0xFF193CB8)
    val AccentHover = Color(0xFF155DFC)
    val Accent2 = PicooCameraColors.Selected
    val Ready = PicooCameraColors.Success
    val Warn = PicooCameraColors.Warning
    val Danger = PicooCameraColors.Danger
    val DangerBg = PicooCameraColors.Danger.copy(alpha = 0.14f)
    val DangerText = Color(0xFFFFB0B8)
    val LiveBg = PicooCameraColors.Surface
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
        fontSize = 28.sp,
        lineHeight = 34.sp,
        letterSpacing = (-0.4).sp,
    ),
    titleLarge = TextStyle(
        fontFamily = PicooFont.Display,
        fontWeight = FontWeight.Bold,
        fontSize = 22.sp,
        lineHeight = 28.sp,
    ),
    titleMedium = TextStyle(
        fontFamily = PicooFont.Display,
        fontWeight = FontWeight.SemiBold,
        fontSize = 17.sp,
        lineHeight = 24.sp,
    ),
    bodyLarge = TextStyle(
        fontFamily = PicooFont.Body,
        fontSize = 16.sp,
        lineHeight = 24.sp,
    ),
    bodyMedium = TextStyle(
        fontFamily = PicooFont.Body,
        fontSize = 14.sp,
        lineHeight = 20.sp,
    ),
    labelLarge = TextStyle(
        fontFamily = PicooFont.Body,
        fontWeight = FontWeight.SemiBold,
        fontSize = 14.sp,
        lineHeight = 20.sp,
    ),
    labelSmall = TextStyle(
        fontFamily = PicooFont.Body,
        fontWeight = FontWeight.SemiBold,
        fontSize = 11.sp,
        lineHeight = 16.sp,
        letterSpacing = 0.3.sp,
    ),
)

private val PicooLightScheme = lightColorScheme(
    primary = PicooLightColors.actionPrimary,
    onPrimary = PicooLightColors.actionOnPrimary,
    secondary = PicooLightColors.actionHighlight,
    background = PicooLightColors.surfacePage,
    surface = PicooLightColors.surfaceGroup,
    surfaceVariant = PicooLightColors.surfaceSubtle,
    onBackground = PicooLightColors.contentPrimary,
    onSurface = PicooLightColors.contentPrimary,
    onSurfaceVariant = PicooLightColors.contentMuted,
    outline = PicooLightColors.borderDefault,
    error = PicooLightColors.statusDanger,
)

private val PicooDarkScheme = darkColorScheme(
    primary = PicooDarkColors.actionPrimary,
    onPrimary = PicooDarkColors.actionOnPrimary,
    secondary = PicooDarkColors.actionHighlight,
    background = PicooDarkColors.surfacePage,
    surface = PicooDarkColors.surfaceGroup,
    surfaceVariant = PicooDarkColors.surfaceSubtle,
    onBackground = PicooDarkColors.contentPrimary,
    onSurface = PicooDarkColors.contentPrimary,
    onSurfaceVariant = PicooDarkColors.contentMuted,
    outline = PicooDarkColors.borderDefault,
    error = PicooDarkColors.statusDanger,
)

@Composable
fun PicooCameraTheme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    content: @Composable () -> Unit,
) {
    val semanticColors = if (darkTheme) PicooDarkColors else PicooLightColors
    androidx.compose.runtime.CompositionLocalProvider(
        LocalPicooColors provides semanticColors,
        LocalPicooDimensions provides PicooDimensions(),
        LocalPicooMotion provides PicooMotion(),
    ) {
        MaterialTheme(
            colorScheme = if (darkTheme) PicooDarkScheme else PicooLightScheme,
            typography = PicooTypography,
            shapes = Shapes(
                small = androidx.compose.foundation.shape.RoundedCornerShape(7.dp),
                medium = androidx.compose.foundation.shape.RoundedCornerShape(15.dp),
                large = androidx.compose.foundation.shape.RoundedCornerShape(15.dp),
            ),
            content = content,
        )
    }
}
