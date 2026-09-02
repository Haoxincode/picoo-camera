package com.picoo.camera.ui.screens

import android.animation.ValueAnimator
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import com.picoo.camera.R
import com.picoo.camera.ui.components.PicooButtonVariant
import com.picoo.camera.ui.components.PicooGhostButton
import com.picoo.camera.ui.components.PicooPrimaryButton
import com.picoo.camera.ui.components.PicooSurfacePanel
import com.picoo.camera.ui.components.Reicon
import com.picoo.camera.ui.components.ReiconIcon
import com.picoo.camera.ui.theme.PicooTheme

@Composable
internal fun DiscoveryHeroPanel(
    searching: Boolean,
    permissionRequired: Boolean,
    onPrimaryAction: () -> Unit,
    onManualConnect: () -> Unit,
    onCheckPermissions: () -> Unit,
    onRequestPermission: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = PicooTheme.colors
    val dimensions = PicooTheme.dimensions
    PicooSurfacePanel(
        modifier = modifier
            .fillMaxWidth()
            .widthIn(max = dimensions.controlContentWidth),
    ) {
        Column(
            modifier = Modifier.padding(
                horizontal = dimensions.panelHorizontalPadding,
                vertical = dimensions.panelVerticalPadding,
            ),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(dimensions.space16),
        ) {
            DiscoveryDeviceIllustration(searching = searching && !permissionRequired)
            Column(
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(dimensions.space8),
                modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite },
            ) {
                Text(
                    text = when {
                        permissionRequired -> "允许查找附近电脑"
                        searching -> "正在查找附近电脑"
                        else -> "未发现附近电脑"
                    },
                    style = MaterialTheme.typography.titleLarge,
                    textAlign = TextAlign.Center,
                )
                Text(
                    text = if (permissionRequired) {
                        "需要附近 Wi‑Fi 设备权限才能自动发现电脑；\n也可以直接输入电脑端显示的 IP 地址。"
                    } else {
                        "请确认电脑端 Picoo Camera 已打开，\n并与手机连接到同一 Wi‑Fi。"
                    },
                    color = colors.contentMuted,
                    style = MaterialTheme.typography.bodyMedium,
                    textAlign = TextAlign.Center,
                )
            }
            if (searching && !permissionRequired) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(dimensions.space8),
                ) {
                    CircularProgressIndicator(
                        modifier = Modifier.size(dimensions.iconStandard),
                        color = colors.actionHighlight,
                        strokeWidth = dimensions.space2,
                    )
                    Text(
                        text = "正在搜索，请稍候…",
                        color = colors.actionHighlight,
                        style = MaterialTheme.typography.labelLarge,
                    )
                }
            }
            PicooPrimaryButton(
                text = when {
                    permissionRequired -> "允许查找电脑"
                    searching -> "停止搜索"
                    else -> "重新搜索附近电脑"
                },
                onClick = if (permissionRequired) onRequestPermission else onPrimaryAction,
                modifier = Modifier.fillMaxWidth(),
                leadingContent = {
                    if (permissionRequired) {
                        ReiconIcon(
                            icon = Reicon.Network,
                            contentDescription = null,
                            modifier = Modifier.size(dimensions.iconEmphasis),
                        )
                    } else if (searching) {
                        StopSearchGlyph()
                    } else {
                        ReiconIcon(
                            icon = Reicon.RefreshDiscovery,
                            contentDescription = null,
                            modifier = Modifier.size(dimensions.iconEmphasis),
                        )
                    }
                },
            )
            PicooGhostButton(
                text = "通过 IP 地址连接",
                onClick = onManualConnect,
                modifier = Modifier.fillMaxWidth(),
                variant = PicooButtonVariant.AccentOutline,
                leadingContent = { IpAddressGlyph() },
            )
            if (!permissionRequired) {
                Surface(
                    onClick = onCheckPermissions,
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(min = dimensions.touchTarget)
                        .semantics { role = Role.Button },
                    color = androidx.compose.ui.graphics.Color.Transparent,
                    contentColor = colors.actionHighlight,
                ) {
                    Row(
                        modifier = Modifier.padding(horizontal = dimensions.space8),
                        horizontalArrangement = Arrangement.Center,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(
                            text = "找不到电脑？检查网络与发现权限",
                            style = MaterialTheme.typography.bodyMedium,
                            textAlign = TextAlign.Center,
                        )
                        ReiconIcon(
                            icon = Reicon.NavigateBack,
                            contentDescription = null,
                            modifier = Modifier
                                .size(dimensions.iconCompact)
                                .graphicsLayer(rotationZ = 180f),
                        )
                    }
                }
            }
        }
    }
}

@Composable
internal fun StopSearchGlyph(modifier: Modifier = Modifier) {
    val dimensions = PicooTheme.dimensions
    val colors = PicooTheme.colors
    Box(
        modifier = modifier
            .size(dimensions.iconEmphasis)
            .border(
                dimensions.space2,
                colors.actionOnPrimary,
                androidx.compose.foundation.shape.CircleShape,
            ),
        contentAlignment = Alignment.Center,
    ) {
        ReiconIcon(
            icon = Reicon.Rejected,
            contentDescription = null,
            modifier = Modifier.size(dimensions.iconCompact),
        )
    }
}

@Composable
internal fun DiscoveryDeviceIllustration(
    searching: Boolean,
    modifier: Modifier = Modifier,
) {
    val colors = PicooTheme.colors
    val dimensions = PicooTheme.dimensions
    val darkTheme = isSystemInDarkTheme()
    Box(
        modifier = modifier
            .fillMaxWidth()
            .height(dimensions.discoveryIllustrationHeight),
        contentAlignment = Alignment.Center,
    ) {
        Box(
            modifier = Modifier
                .size(dimensions.discoveryIllustrationHeight)
                .background(colors.surfaceAccent, androidx.compose.foundation.shape.CircleShape),
        )
        if (searching && ValueAnimator.areAnimatorsEnabled()) {
            AnimatedDiscoveryOrbit()
        } else {
            StaticDiscoveryOrbit()
        }
        Image(
            painter = painterResource(
                if (darkTheme) R.drawable.picoo_macbook_pro_dark
                else R.drawable.picoo_macbook_pro_light,
            ),
            contentDescription = null,
            contentScale = ContentScale.Fit,
            modifier = Modifier
                .align(Alignment.BottomCenter)
                .width(dimensions.discoveryDeviceWidth)
                .aspectRatio(5f / 3f),
        )
        ReiconIcon(
            icon = Reicon.Network,
            contentDescription = null,
            tint = colors.actionHighlight,
            modifier = Modifier
                .align(Alignment.TopCenter)
                .padding(top = dimensions.space24)
                .size(dimensions.iconHero),
        )
    }
}

@Composable
internal fun AnimatedDiscoveryOrbit(modifier: Modifier = Modifier) {
    val dimensions = PicooTheme.dimensions
    val transition = rememberInfiniteTransition(label = "receiverDiscovery")
    val rotation by transition.animateFloat(
        initialValue = 0f,
        targetValue = 360f,
        animationSpec = infiniteRepeatable(
            animation = tween(
                durationMillis = PicooTheme.motion.deliberateMillis * 4,
                easing = LinearEasing,
            ),
        ),
        label = "receiverDiscoveryOrbit",
    )
    DiscoveryOrbit(
        modifier = modifier.graphicsLayer(rotationZ = rotation),
        showArc = true,
    )
}

@Composable
internal fun StaticDiscoveryOrbit(modifier: Modifier = Modifier) {
    DiscoveryOrbit(modifier = modifier, showArc = false)
}

@Composable
internal fun DiscoveryOrbit(
    showArc: Boolean,
    modifier: Modifier = Modifier,
) {
    val colors = PicooTheme.colors
    val dimensions = PicooTheme.dimensions
    Canvas(
        modifier = modifier.size(dimensions.discoveryIllustrationHeight - dimensions.space16),
    ) {
        val stroke = dimensions.space2.toPx()
        drawCircle(
            color = colors.borderAccent,
            style = Stroke(width = stroke),
        )
        drawCircle(
            color = colors.borderAccent.copy(alpha = 0.7f),
            radius = size.minDimension / 2 - dimensions.space24.toPx(),
            style = Stroke(width = stroke),
        )
        if (showArc) {
            drawArc(
                color = colors.actionHighlight.copy(alpha = 0.42f),
                startAngle = -32f,
                sweepAngle = 84f,
                useCenter = false,
                style = Stroke(width = dimensions.space8.toPx(), cap = StrokeCap.Round),
            )
        }
    }
}

@Composable
internal fun IpAddressGlyph(modifier: Modifier = Modifier) {
    val colors = PicooTheme.colors
    val dimensions = PicooTheme.dimensions
    Box(
        modifier = modifier
            .size(dimensions.iconEmphasis)
            .border(
                width = dimensions.space2 / 2,
                color = colors.actionHighlight,
                shape = RoundedCornerShape(dimensions.radiusControl),
            ),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = "IP",
            style = MaterialTheme.typography.labelSmall,
            fontWeight = FontWeight.Bold,
        )
    }
}
