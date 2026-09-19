package com.picoo.camera.ui.screens

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Arrangement
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
import com.picoo.camera.ui.formatPairingCode
import com.picoo.camera.ui.components.PicooButtonSize
import com.picoo.camera.ui.components.PicooGhostButton
import com.picoo.camera.ui.components.PicooPrimaryButton
import com.picoo.camera.ui.components.PicooVisualContext
import com.picoo.camera.ui.components.Reicon
import com.picoo.camera.ui.components.ReiconIcon
import com.picoo.camera.ui.theme.PicooFont
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
        text = "设备温度较高，请注意散热或停止推流",
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

/** Pairing and confirmation stay on the camera surface as transient overlays. */
@Composable
internal fun PairingCameraOverlay(
    receiverName: String,
    pairingCode: String,
    remainingSeconds: Int,
    expired: Boolean,
    errorText: String?,
    onConfirm: () -> Unit,
    onRegenerate: () -> Unit,
    onCancel: () -> Unit,
) {
    val dimensions = PicooTheme.dimensions
    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(PicooCameraColors.ReconnectScrim)
            .padding(dimensions.space24),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(dimensions.radiusHero))
                .background(PicooCameraColors.SurfaceRaised)
                .padding(dimensions.space24),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(dimensions.space12),
        ) {
            ReiconIcon(
                icon = Reicon.SecureConnection,
                contentDescription = null,
                tint = PicooCameraColors.Selected,
                modifier = Modifier.size(dimensions.iconHero),
            )
            Text(
                text = receiverName.ifBlank { "电脑" },
                color = PicooCameraColors.ContentMuted,
                style = PicooCameraTypography.Status,
            )
            Text(
                text = "核对 6 位配对短码",
                color = PicooCameraColors.Content,
                style = PicooCameraTypography.OverlayTitle,
            )
            Text(
                text = formatPairingCode(pairingCode).ifBlank { "··· ···" },
                color = if (expired) PicooCameraColors.ContentMuted else PicooCameraColors.Selected,
                fontFamily = PicooFont.Mono,
                style = PicooCameraTypography.PairingCode,
            )
            Text(
                text = if (expired) "短码已过期" else "剩余 ${remainingSeconds.coerceAtLeast(0)} 秒",
                color = if (expired) PicooCameraColors.Danger else PicooCameraColors.ContentMuted,
                style = PicooCameraTypography.Status,
            )
            Text(
                text = if (expired) {
                    "60 秒内未完成双向确认，请重新发起配对。"
                } else {
                    "确认手机与电脑屏幕上显示相同数字；不一致时请立即取消。"
                },
                color = PicooCameraColors.ContentMuted,
                style = PicooCameraTypography.Status,
                textAlign = TextAlign.Center,
            )
            if (expired) {
                PicooPrimaryButton(
                    text = "重新发起配对",
                    onClick = onRegenerate,
                    modifier = Modifier.fillMaxWidth(),
                    context = PicooVisualContext.Camera,
                )
            } else {
                PicooPrimaryButton(
                    text = "两端数字一致，确认配对",
                    onClick = onConfirm,
                    enabled = pairingCode.isNotEmpty(),
                    modifier = Modifier.fillMaxWidth(),
                    context = PicooVisualContext.Camera,
                )
            }
            PicooGhostButton(
                text = "取消",
                onClick = onCancel,
                modifier = Modifier.fillMaxWidth(),
                context = PicooVisualContext.Camera,
            )
            errorText?.let { message ->
                Text(
                    text = message,
                    color = PicooCameraColors.Danger,
                    style = PicooCameraTypography.Status,
                    textAlign = TextAlign.Center,
                )
            }
        }
    }
}

@Composable
internal fun PairingWaitCameraOverlay(
    receiverName: String,
    outcome: WaitOutcome,
    onCancel: () -> Unit,
    onRetry: () -> Unit,
) {
    val dimensions = PicooTheme.dimensions
    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(PicooCameraColors.ReconnectScrim)
            .padding(dimensions.space24),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(dimensions.radiusHero))
                .background(PicooCameraColors.SurfaceRaised)
                .padding(dimensions.space24),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(dimensions.space12),
        ) {
            if (outcome == WaitOutcome.Pending) {
                CircularProgressIndicator(
                    color = PicooCameraColors.Selected,
                    modifier = Modifier.size(dimensions.iconHero),
                )
                Text("等待电脑端确认", color = PicooCameraColors.Content, style = PicooCameraTypography.OverlayTitle)
                Text(
                    "请在 $receiverName 的配对提示中确认相同短码。双方确认后会自动开始推流。",
                    color = PicooCameraColors.ContentMuted,
                    style = PicooCameraTypography.Status,
                    textAlign = TextAlign.Center,
                )
                PicooGhostButton(
                    text = "取消连接",
                    onClick = onCancel,
                    modifier = Modifier.fillMaxWidth(),
                    context = PicooVisualContext.Camera,
                )
            } else {
                Text(
                    text = if (outcome == WaitOutcome.Rejected) "电脑端拒绝了连接" else "配对已超时",
                    color = PicooCameraColors.Content,
                    style = PicooCameraTypography.OverlayTitle,
                )
                Text(
                    text = if (outcome == WaitOutcome.Rejected) {
                        "确认电脑归属后可重新发起连接。"
                    } else {
                        "本次短码已失效，请重新发起连接。"
                    },
                    color = PicooCameraColors.ContentMuted,
                    style = PicooCameraTypography.Status,
                    textAlign = TextAlign.Center,
                )
                PicooPrimaryButton(
                    text = "重新发起连接",
                    onClick = onRetry,
                    modifier = Modifier.fillMaxWidth(),
                    context = PicooVisualContext.Camera,
                )
                PicooGhostButton(
                    text = "取消",
                    onClick = onCancel,
                    modifier = Modifier.fillMaxWidth(),
                    context = PicooVisualContext.Camera,
                )
            }
        }
    }
}
