package com.picoo.camera.ui.components

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.scale
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import com.picoo.camera.ui.theme.PicooCameraColors
import com.picoo.camera.ui.theme.PicooTheme

enum class PicooVisualContext { Control, Camera }

enum class PicooButtonVariant { Neutral, AccentOutline }

enum class PicooButtonSize { Standard, Compact }

/** Shared control-context panel. Callers own layout; the component owns surface treatment. */
@Composable
fun PicooSurfacePanel(
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit,
) {
    val colors = PicooTheme.colors
    val dimensions = PicooTheme.dimensions
    Surface(
        modifier = modifier,
        color = colors.surfaceGroup,
        shape = RoundedCornerShape(dimensions.radiusHero),
        border = BorderStroke(dimensions.borderHairline, colors.borderDefault),
        shadowElevation = dimensions.elevationSurface,
        content = content,
    )
}

@Composable
fun PicooPrimaryButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    loading: Boolean = false,
    context: PicooVisualContext = PicooVisualContext.Control,
    leadingContent: (@Composable () -> Unit)? = null,
) {
    val dimensions = PicooTheme.dimensions
    val colors = PicooTheme.colors
    val progressColor = if (context == PicooVisualContext.Camera) {
        PicooCameraColors.Content
    } else {
        colors.actionOnPrimary
    }
    Button(
        onClick = onClick,
        modifier = modifier
            .defaultMinSize(minHeight = dimensions.actionButtonHeight)
            .semantics {
                if (loading) stateDescription = "正在处理"
            },
        enabled = enabled && !loading,
        shape = RoundedCornerShape(dimensions.radiusAction),
        contentPadding = PaddingValues(horizontal = dimensions.space16, vertical = dimensions.space12),
        elevation = ButtonDefaults.buttonElevation(defaultElevation = 0.dp),
        colors = ButtonDefaults.buttonColors(
            containerColor = if (context == PicooVisualContext.Camera) {
                PicooCameraColors.Selected
            } else {
                colors.actionPrimary
            },
            contentColor = if (context == PicooVisualContext.Camera) {
                PicooCameraColors.Content
            } else {
                colors.actionOnPrimary
            },
            disabledContainerColor = if (context == PicooVisualContext.Camera) {
                PicooCameraColors.Control
            } else {
                colors.surfaceSubtle
            },
            disabledContentColor = if (context == PicooVisualContext.Camera) {
                PicooCameraColors.ContentMuted
            } else {
                colors.contentMuted
            },
        ),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            when {
                loading -> CircularProgressIndicator(
                    modifier = Modifier.size(dimensions.iconStandard),
                    color = progressColor,
                    strokeWidth = dimensions.space2,
                )
                leadingContent != null -> leadingContent()
            }
            if (loading || leadingContent != null) {
                Spacer(modifier = Modifier.size(dimensions.space8))
            }
            Text(text = text, style = MaterialTheme.typography.labelLarge)
        }
    }
}

@Composable
fun PicooGhostButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    variant: PicooButtonVariant = PicooButtonVariant.Neutral,
    size: PicooButtonSize = PicooButtonSize.Standard,
    enabled: Boolean = true,
    loading: Boolean = false,
    context: PicooVisualContext = PicooVisualContext.Control,
    leadingContent: (@Composable () -> Unit)? = null,
) {
    val dimensions = PicooTheme.dimensions
    val colors = PicooTheme.colors
    OutlinedButton(
        onClick = onClick,
        modifier = modifier
            .defaultMinSize(
                minHeight = if (size == PicooButtonSize.Compact) {
                    dimensions.touchTarget
                } else {
                    dimensions.actionButtonHeight
                },
            )
            .semantics {
                if (loading) stateDescription = "正在处理"
            },
        enabled = enabled && !loading,
        shape = RoundedCornerShape(dimensions.radiusAction),
        contentPadding = PaddingValues(
            horizontal = dimensions.space16,
            vertical = if (size == PicooButtonSize.Compact) dimensions.space8 else dimensions.space12,
        ),
        colors = ButtonDefaults.outlinedButtonColors(
            contentColor = if (context == PicooVisualContext.Camera) {
                PicooCameraColors.Content
            } else if (variant == PicooButtonVariant.AccentOutline) {
                colors.actionHighlight
            } else {
                colors.contentPrimary
            },
        ),
        border = BorderStroke(
            dimensions.borderHairline,
            if (context == PicooVisualContext.Camera) {
                PicooCameraColors.ControlBorder
            } else if (variant == PicooButtonVariant.AccentOutline) {
                colors.actionHighlight
            } else {
                colors.borderDefault
            },
        ),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            when {
                loading -> CircularProgressIndicator(
                    modifier = Modifier.size(dimensions.iconStandard),
                    color = if (context == PicooVisualContext.Camera) {
                        PicooCameraColors.Content
                    } else if (variant == PicooButtonVariant.AccentOutline) {
                        colors.actionHighlight
                    } else {
                        colors.contentPrimary
                    },
                    strokeWidth = dimensions.space2,
                )
                leadingContent != null -> leadingContent()
            }
            if (loading || leadingContent != null) {
                Spacer(modifier = Modifier.size(dimensions.space8))
            }
            Text(text = text, style = MaterialTheme.typography.labelLarge)
        }
    }
}

@Composable
fun PicooDangerButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    armed: Boolean = false,
) {
    val dimensions = PicooTheme.dimensions
    val colors = PicooTheme.colors
    OutlinedButton(
        onClick = onClick,
        modifier = modifier.defaultMinSize(minHeight = dimensions.touchTarget),
        shape = RoundedCornerShape(dimensions.radiusControl),
        colors = ButtonDefaults.outlinedButtonColors(
            containerColor = if (armed) colors.statusDanger else Color.Transparent,
            contentColor = if (armed) colors.actionOnPrimary else colors.statusDanger,
        ),
        border = BorderStroke(dimensions.borderHairline, colors.statusDanger),
    ) {
        Text(text = text, style = MaterialTheme.typography.labelLarge)
    }
}

@Composable
fun PicooIconButton(
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    contentDescription: String? = null,
    context: PicooVisualContext = PicooVisualContext.Control,
    content: @Composable () -> Unit,
) {
    val dimensions = PicooTheme.dimensions
    val colors = PicooTheme.colors
    IconButton(
        onClick = onClick,
        modifier = modifier
            .size(dimensions.touchTarget)
            .then(
                if (contentDescription == null) {
                    Modifier
                } else {
                    Modifier.semantics { this.contentDescription = contentDescription }
                },
            ),
        colors = IconButtonDefaults.iconButtonColors(
            contentColor = if (context == PicooVisualContext.Camera) {
                PicooCameraColors.Content
            } else {
                colors.contentPrimary
            },
        ),
        content = content,
    )
}

@Composable
fun PicooPill(
    text: String,
    modifier: Modifier = Modifier,
    warn: Boolean = false,
    neutral: Boolean = false,
) {
    val colors = PicooTheme.colors
    val dimensions = PicooTheme.dimensions
    val foreground = when {
        warn -> colors.statusWarning
        neutral -> colors.contentMuted
        else -> colors.statusSuccess
    }
    Text(
        text = text,
        modifier = modifier
            .background(foreground.copy(alpha = 0.10f), CircleShape)
            .padding(horizontal = dimensions.space8, vertical = dimensions.space4),
        color = foreground,
        style = MaterialTheme.typography.labelSmall,
    )
}

@Composable
fun DeviceBadge(label: String, paired: Boolean, offline: Boolean = false) {
    val colors = PicooTheme.colors
    val foreground = when {
        offline -> colors.contentMuted
        paired -> colors.actionHighlight
        else -> colors.statusSuccess
    }
    Text(
        text = label,
        modifier = Modifier
            .background(foreground.copy(alpha = 0.10f), CircleShape)
            .padding(horizontal = PicooTheme.dimensions.space8, vertical = PicooTheme.dimensions.space2),
        color = foreground,
        style = MaterialTheme.typography.labelSmall,
    )
}

@Composable
fun DiscoveryPulseDot(searching: Boolean) {
    val colors = PicooTheme.colors
    val transition = rememberInfiniteTransition(label = "discovery")
    val alpha by transition.animateFloat(
        initialValue = if (searching) 0.42f else 1f,
        targetValue = 1f,
        animationSpec = infiniteRepeatable(
            animation = tween(PicooTheme.motion.deliberateMillis * 2, easing = LinearEasing),
            repeatMode = RepeatMode.Reverse,
        ),
        label = "discoveryAlpha",
    )
    Box(
        modifier = Modifier
            .size(PicooTheme.dimensions.space8)
            .alpha(if (searching) alpha else 1f)
            .scale(if (searching) 1f else 0.9f)
            .background(
                if (searching) colors.actionHighlight else colors.statusSuccess,
                CircleShape,
            ),
    )
}

/** Native Material bottom sheet for AC-M-DISC-03 / AC-M-SET-02. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PicooSheet(
    title: String,
    onDismiss: () -> Unit,
    modifier: Modifier = Modifier,
    description: String? = null,
    content: @Composable ColumnScope.() -> Unit,
) {
    val dimensions = PicooTheme.dimensions
    val colors = PicooTheme.colors
    ModalBottomSheet(
        modifier = modifier,
        onDismissRequest = onDismiss,
        containerColor = colors.surfaceGroup,
        contentColor = colors.contentPrimary,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = dimensions.space16)
                .padding(bottom = dimensions.space24)
                .imePadding()
                .verticalScroll(rememberScrollState()),
        ) {
            Text(text = title, style = MaterialTheme.typography.titleMedium)
            if (description != null) {
                Text(
                    text = description,
                    color = colors.contentMuted,
                    style = MaterialTheme.typography.bodyMedium,
                    modifier = Modifier.padding(top = dimensions.space4, bottom = dimensions.space12),
                )
            } else {
                Spacer(modifier = Modifier.height(dimensions.space8))
            }
            content()
        }
    }
}

@Composable
fun PicooSheetRow(
    title: String,
    subtitle: String? = null,
    selected: Boolean = false,
    danger: Boolean = false,
    onClick: () -> Unit,
) {
    val colors = PicooTheme.colors
    val dimensions = PicooTheme.dimensions
    Surface(
        onClick = onClick,
        modifier = Modifier
            .fillMaxWidth()
            .defaultMinSize(minHeight = dimensions.touchTarget)
            .semantics { role = Role.Button },
        color = if (selected) colors.actionHighlight.copy(alpha = 0.10f) else Color.Transparent,
        contentColor = if (danger) colors.statusDanger else colors.contentPrimary,
    ) {
        Column(
            modifier = Modifier.padding(horizontal = dimensions.space8, vertical = dimensions.space12),
        ) {
            Text(text = title, style = MaterialTheme.typography.bodyLarge)
            if (subtitle != null) {
                Text(
                    text = subtitle,
                    color = if (danger) colors.statusDanger else colors.contentMuted,
                    style = MaterialTheme.typography.bodyMedium,
                    modifier = Modifier.padding(top = dimensions.space2),
                )
            }
        }
    }
    HorizontalDivider(color = colors.borderDefault)
}
