package com.picoo.camera.ui.screens

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.scale
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.IntOffset
import com.picoo.camera.ui.ReconnectBackoffFormat
import com.picoo.camera.ui.components.PicooButtonSize
import com.picoo.camera.ui.components.PicooGhostButton
import com.picoo.camera.ui.components.PicooVisualContext
import com.picoo.camera.ui.theme.PicooCameraColors
import com.picoo.camera.ui.theme.PicooCameraDimensions
import com.picoo.camera.ui.theme.PicooCameraTypography
import com.picoo.camera.ui.theme.PicooTheme
import kotlin.math.roundToInt

@Composable
internal fun PowerHint(text: String) {
    val dimensions = PicooTheme.dimensions
    Text(
        text = text,
        color = PicooCameraColors.Warning,
        style = PicooCameraTypography.Status,
        textAlign = TextAlign.Center,
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(dimensions.radiusControl))
            .background(PicooCameraColors.WarningSurface)
            .border(
                PicooCameraDimensions.Border,
                PicooCameraColors.WarningBorder,
                RoundedCornerShape(dimensions.radiusControl),
            )
            .padding(dimensions.space8),
    )
}

@Composable
internal fun ThermalBanner() {
    val dimensions = PicooTheme.dimensions
    Text(
        text = "设备偏热保护：已自动降至 720P 稳住帧率，1080P 暂不可选",
        color = PicooCameraColors.WarningContent,
        style = PicooCameraTypography.Status,
        textAlign = TextAlign.Center,
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(dimensions.radiusControl))
            .background(PicooCameraColors.WarningSurface)
            .border(
                PicooCameraDimensions.Border,
                PicooCameraColors.WarningBorder,
                RoundedCornerShape(dimensions.radiusControl),
            )
            .padding(dimensions.space8),
    )
}

@Composable
internal fun CameraToast(text: String, modifier: Modifier = Modifier) {
    val dimensions = PicooTheme.dimensions
    Text(
        text = text,
        color = PicooCameraColors.WarningContent,
        style = PicooCameraTypography.Status,
        modifier = modifier
            .statusBarsPadding()
            .padding(top = PicooCameraDimensions.ToastTopInset)
            .clip(RoundedCornerShape(dimensions.radiusControl))
            .background(PicooCameraColors.WarningToastSurface)
            .border(
                PicooCameraDimensions.Border,
                PicooCameraColors.WarningBorder,
                RoundedCornerShape(dimensions.radiusControl),
            )
            .padding(horizontal = dimensions.space12, vertical = dimensions.space8),
    )
}

@Composable
internal fun FocusRing(center: Offset) {
    val dimensions = PicooTheme.dimensions
    val shrink by animateFloatAsState(
        targetValue = PicooCameraDimensions.FocusRingTargetScale,
        animationSpec = tween(durationMillis = PicooTheme.motion.fastMillis),
        label = "focusShrink",
    )
    val halfPx = with(LocalDensity.current) { PicooCameraDimensions.FocusRingSize.toPx() / 2f }
    Box(
        modifier = Modifier
            .offset {
                IntOffset(
                    (center.x - halfPx).roundToInt(),
                    (center.y - halfPx).roundToInt(),
                )
            }
            .size(PicooCameraDimensions.FocusRingSize)
            .scale(shrink)
            .border(
                PicooCameraDimensions.BorderEmphasis,
                PicooCameraColors.Focus,
                RoundedCornerShape(dimensions.radiusControl),
            ),
    )
}

@Composable
internal fun ReconnectOverlay(
    networkUnstable: Boolean,
    reconnectAttempt: Int,
    reconnectDelayMs: Long,
    onStopReconnect: () -> Unit,
) {
    val dimensions = PicooTheme.dimensions
    val title = if (networkUnstable) "网络不稳定，正在优化…" else "网络中断，正在重连…"
    val detail = if (networkUnstable) {
        "链路丢包较高，保持推流并等待恢复\n连接恢复后将自动请求 IDR 关键帧"
    } else {
        ReconnectBackoffFormat.detailMessage(reconnectAttempt, reconnectDelayMs)
    }
    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(PicooCameraColors.ReconnectScrim),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            modifier = Modifier.padding(dimensions.space24),
        ) {
            CircularProgressIndicator(
                color = PicooCameraColors.Selected,
                strokeWidth = PicooCameraDimensions.ProgressStroke,
                modifier = Modifier.size(PicooCameraDimensions.ProgressSize),
            )
            Spacer(modifier = Modifier.height(dimensions.space16))
            Text(
                text = title,
                color = PicooCameraColors.Content,
                style = PicooCameraTypography.OverlayTitle,
            )
            Spacer(modifier = Modifier.height(dimensions.space8))
            Text(
                text = detail,
                color = PicooCameraColors.ContentMuted,
                style = PicooCameraTypography.Status,
                textAlign = TextAlign.Center,
            )
            Spacer(modifier = Modifier.height(dimensions.space16))
            PicooGhostButton(
                text = "停止重连并退出",
                onClick = onStopReconnect,
                size = PicooButtonSize.Compact,
                context = PicooVisualContext.Camera,
            )
        }
    }
}
