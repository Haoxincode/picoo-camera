package com.picoo.camera.ui.screens

import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.blur
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.draw.scale
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.IntOffset
import kotlin.math.roundToInt
import kotlinx.coroutines.delay
import com.picoo.camera.media.LensFacing
import com.picoo.camera.ui.CameraPreviewSurface
import com.picoo.camera.ui.ExposurePreview
import com.picoo.camera.ui.components.PicooGhostButton
import com.picoo.camera.ui.components.PicooIconButton
import com.picoo.camera.ui.components.PicooPrimaryButton
import com.picoo.camera.ui.components.PicooVisualContext
import com.picoo.camera.ui.components.Reicon
import com.picoo.camera.ui.components.ReiconIcon
import com.picoo.camera.ui.ReconnectBackoffFormat
import com.picoo.camera.ui.theme.PicooCameraColors
import com.picoo.camera.ui.theme.PicooCameraDimensions
import com.picoo.camera.ui.theme.PicooCameraTypography
import com.picoo.camera.ui.theme.PicooFont
import com.picoo.camera.ui.theme.PicooTheme

/** REQ-PICOO-UI-003 / REQ-PICOO-UI-005 — 传输页，对齐 m-screen-streaming。 */
@Composable
fun StreamingScreen(
    cameraGranted: Boolean,
    receiverName: String,
    linkQualityChip: String,
    resolutionLabel: String,
    bitrateMbps: String,
    lensFacing: LensFacing,
    previewBufferWidth: Int,
    previewBufferHeight: Int,
    previewSensorOrientationDegrees: Int,
    previewFrontFacing: Boolean,
    localPreviewMirrored: Boolean,
    thermalForced720: Boolean,
    powerHint: String,
    reconnecting: Boolean,
    packetLossLabel: String,
    onRequestCamera: () -> Unit,
    onFlipCamera: () -> Unit,
    onToggleResolution: () -> Unit,
    onToggleMirror: () -> Unit,
    onEvMinus: () -> Unit,
    onEvPlus: () -> Unit,
    onEvReset: () -> Unit,
    exposureEv: Int,
    evSupported: Boolean,
    onDisconnect: () -> Unit,
    onStopReconnect: () -> Unit,
    onPreviewSurfaceAvailable: (android.graphics.SurfaceTexture) -> Unit,
    onPreviewSurfaceDestroyed: (android.graphics.SurfaceTexture) -> Unit,
    onPreviewDisplayChanged: () -> Unit,
    modifier: Modifier = Modifier,
    networkUnstable: Boolean = false,
    reconnectAttempt: Int = 0,
    reconnectDelayMs: Long = 0L,
) {
    val dimensions = PicooTheme.dimensions
    val motion = PicooTheme.motion
    var uiLocked by remember { mutableStateOf(false) }
    var showEvPanel by remember { mutableStateOf(false) }
    var shutterArmed by remember { mutableStateOf(false) }
    var flipRotationTarget by remember { mutableFloatStateOf(0f) }
    var flipBlurActive by remember { mutableStateOf(false) }
    var thermalToast by remember { mutableStateOf(false) }
    var immersive by remember { mutableStateOf(false) }
    var focusRingCenter by remember { mutableStateOf(Offset.Zero) }
    var focusRingActive by remember { mutableStateOf(false) }
    val flipRotation by animateFloatAsState(
        targetValue = flipRotationTarget,
        animationSpec = tween(durationMillis = motion.normalMillis),
        label = "flipRotation",
    )

    LaunchedEffect(shutterArmed) {
        if (shutterArmed) {
            delay(3_000)
            shutterArmed = false
        }
    }

    LaunchedEffect(focusRingActive) {
        if (focusRingActive) {
            delay(800)
            focusRingActive = false
        }
    }

    LaunchedEffect(flipBlurActive) {
        if (flipBlurActive) {
            delay(280)
            flipBlurActive = false
        }
    }

    LaunchedEffect(thermalToast) {
        if (thermalToast) {
            delay(2_400)
            thermalToast = false
        }
    }

    LaunchedEffect(uiLocked) {
        if (uiLocked) {
            immersive = false
        }
    }

    val showChrome = !uiLocked && !immersive

    Box(
        modifier = modifier
            .fillMaxSize()
            .background(PicooCameraColors.Surface),
    ) {
        if (cameraGranted) {
            CameraPreviewSurface(
                modifier = Modifier.fillMaxSize(),
                bufferWidth = previewBufferWidth,
                bufferHeight = previewBufferHeight,
                sensorOrientationDegrees = previewSensorOrientationDegrees,
                frontFacing = previewFrontFacing,
                mirrorLocal = localPreviewMirrored,
                onSurfaceAvailable = onPreviewSurfaceAvailable,
                onSurfaceDestroyed = onPreviewSurfaceDestroyed,
                onDisplayChanged = onPreviewDisplayChanged,
            )
            val evAlpha = ExposurePreview.overlayAlpha(exposureEv)
            if (evAlpha > 0f) {
                Box(
                    modifier = Modifier
                        .fillMaxSize()
                        .background(ExposurePreview.overlayColor(exposureEv).copy(alpha = evAlpha)),
                )
            }
            if (flipBlurActive) {
                Box(
                    modifier = Modifier
                        .fillMaxSize()
                        .blur(PicooCameraDimensions.FlipBlur)
                        .background(PicooCameraColors.TransitionScrim),
                )
            }
        } else {
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .background(PicooCameraColors.SurfaceRaised),
                contentAlignment = Alignment.Center,
            ) {
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Text(
                        text = "需要相机权限才能预览与推流",
                        color = PicooCameraColors.ContentMuted,
                        style = PicooCameraTypography.Status,
                    )
                    Spacer(modifier = Modifier.height(dimensions.space12))
                    PicooPrimaryButton(
                        text = "启用相机",
                        onClick = onRequestCamera,
                        modifier = Modifier.padding(horizontal = dimensions.space32),
                        context = PicooVisualContext.Camera,
                    )
                }
            }
        }

        if (cameraGranted && !uiLocked) {
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .pointerInput(Unit) {
                        detectTapGestures(
                            onTap = { offset ->
                                focusRingCenter = offset
                                focusRingActive = true
                            },
                            onDoubleTap = { immersive = !immersive },
                        )
                    },
            )
        }

        if (showChrome) {
            SafeFrameOverlay()
        }

        if (focusRingActive) {
            FocusRing(center = focusRingCenter)
        }

        if (showChrome) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .statusBarsPadding()
                    .padding(
                        start = dimensions.space12,
                        end = dimensions.space12,
                        top = dimensions.space8,
                    ),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                HudBadge {
                    Box(
                        modifier = Modifier
                            .size(PicooCameraDimensions.StatusDot)
                            .background(PicooCameraColors.Success, CircleShape),
                    )
                    Text(
                        text = receiverName.ifBlank { "电脑" },
                        color = PicooCameraColors.Content,
                        style = PicooCameraTypography.Caption,
                        fontWeight = FontWeight.SemiBold,
                    )
                    val latency = linkQualityChip.substringAfter(" · ", "")
                    if (latency.isNotEmpty()) {
                        Text(
                            text = latency,
                            color = PicooCameraColors.Success,
                            style = PicooCameraTypography.Label.copy(fontFamily = PicooFont.Mono),
                        )
                    }
                }

                Row(
                    modifier = Modifier
                        .clip(RoundedCornerShape(PicooCameraDimensions.PillRadius))
                        .background(PicooCameraColors.ToolbarOverlay)
                        .border(
                            PicooCameraDimensions.Border,
                            PicooCameraColors.ControlBorderMuted,
                            RoundedCornerShape(PicooCameraDimensions.PillRadius),
                        )
                        .padding(horizontal = dimensions.space4, vertical = dimensions.space2),
                    horizontalArrangement = Arrangement.spacedBy(dimensions.space2),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    CamToolButton(
                        onClick = { showEvPanel = !showEvPanel },
                        enabled = evSupported,
                        active = showEvPanel,
                        contentDescription = "曝光补偿",
                    ) {
                        ReiconIcon(
                            icon = Reicon.Exposure,
                            contentDescription = null,
                            tint = if (showEvPanel) PicooCameraColors.Selected else PicooCameraColors.ContentMuted,
                            modifier = Modifier.size(PicooCameraDimensions.ToolIcon),
                        )
                    }
                    CamToolButton(
                        onClick = onToggleMirror,
                        active = localPreviewMirrored,
                        contentDescription = "预览镜像",
                    ) {
                        ReiconIcon(
                            icon = Reicon.Mirror,
                            contentDescription = null,
                            tint = if (localPreviewMirrored) PicooCameraColors.Selected else PicooCameraColors.ContentMuted,
                            modifier = Modifier.size(PicooCameraDimensions.ToolIcon),
                        )
                    }
                    CamToolButton(
                        onClick = { uiLocked = !uiLocked },
                        active = uiLocked,
                        contentDescription = "防误触锁定",
                    ) {
                        ReiconIcon(
                            icon = if (uiLocked) Reicon.InteractionLock else Reicon.InteractionUnlock,
                            contentDescription = null,
                            tint = if (uiLocked) PicooCameraColors.Selected else PicooCameraColors.ContentMuted,
                            modifier = Modifier.size(PicooCameraDimensions.ToolIcon),
                        )
                    }
                }

                ResPill(
                    text = "${resolutionLabel.uppercase()} · 30",
                    throttled = thermalForced720,
                    onClick = {
                        if (thermalForced720 &&
                            resolutionLabel.contains("720", ignoreCase = true)
                        ) {
                            thermalToast = true
                        } else {
                            onToggleResolution()
                        }
                    },
                )
            }
        }

        if (showChrome) {
            Column(
                modifier = Modifier
                    .align(Alignment.BottomCenter)
                    .fillMaxWidth()
                    .background(
                        Brush.verticalGradient(
                            colors = listOf(
                                PicooCameraColors.Surface.copy(alpha = 0f),
                                PicooCameraColors.BottomScrim,
                            ),
                        ),
                    )
                    .navigationBarsPadding()
                    .padding(
                        start = dimensions.space16,
                        end = dimensions.space16,
                        top = dimensions.space8,
                        bottom = dimensions.space24,
                    ),
            ) {
                if (thermalForced720) {
                    ThermalBanner()
                    Spacer(modifier = Modifier.height(dimensions.space8))
                } else if (powerHint.isNotEmpty()) {
                    Text(
                        text = powerHint,
                        color = PicooCameraColors.Warning,
                        style = PicooCameraTypography.Status,
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
                    Spacer(modifier = Modifier.height(dimensions.space8))
                }

                if (showEvPanel && evSupported) {
                    EvPanel(
                        exposureEv = exposureEv,
                        onEvMinus = onEvMinus,
                        onEvPlus = onEvPlus,
                        onEvReset = onEvReset,
                    )
                    Spacer(modifier = Modifier.height(dimensions.space8))
                }

                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    StatPill(bitrate = bitrateMbps, packetLossLabel = packetLossLabel)

                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        val infinite = rememberInfiniteTransition(label = "shutterPulse")
                        val pulseScale by infinite.animateFloat(
                            initialValue = 1f,
                            targetValue = if (shutterArmed) 1.06f else 1f,
                            animationSpec = infiniteRepeatable(
                                animation = tween(durationMillis = motion.deliberateMillis),
                                repeatMode = RepeatMode.Reverse,
                            ),
                            label = "shutterScale",
                        )
                        Box(
                            modifier = Modifier
                                .size(dimensions.cameraStopTarget)
                                .scale(if (shutterArmed) pulseScale else 1f)
                                .clip(CircleShape)
                                .border(
                                    width = dimensions.cameraStopStroke,
                                    color = if (shutterArmed) PicooCameraColors.Danger else PicooCameraColors.StopBorder,
                                    shape = CircleShape,
                                )
                                .background(PicooCameraColors.StopSurface)
                                .semantics {
                                    contentDescription = "断开连接"
                                    stateDescription = if (shutterArmed) "等待再次确认" else "未确认"
                                    role = Role.Button
                                }
                                .clickable(role = Role.Button) {
                                    if (shutterArmed) {
                                        onDisconnect()
                                    } else {
                                        shutterArmed = true
                                    }
                                },
                            contentAlignment = Alignment.Center,
                        ) {
                            ReiconIcon(
                                icon = Reicon.StopStream,
                                contentDescription = null,
                                tint = if (shutterArmed) PicooCameraColors.DangerEmphasis else PicooCameraColors.Danger,
                                modifier = Modifier.size(dimensions.iconEmphasis),
                            )
                        }
                        Spacer(modifier = Modifier.height(dimensions.space4))
                        Text(
                            text = if (shutterArmed) "再次点击确认断开" else "断开连接",
                            color = if (shutterArmed) PicooCameraColors.DangerContent else PicooCameraColors.ContentMuted,
                            style = PicooCameraTypography.Label,
                            fontWeight = if (shutterArmed) FontWeight.Bold else FontWeight.SemiBold,
                        )
                    }

                    Box(
                        modifier = Modifier
                            .size(dimensions.touchTarget)
                            .clip(CircleShape)
                            .border(
                                PicooCameraDimensions.Border,
                                PicooCameraColors.ControlBorder,
                                CircleShape,
                            )
                            .background(PicooCameraColors.ControlSurfaceSubtle)
                            .semantics {
                                contentDescription = "切换前后摄像头"
                                role = Role.Button
                            }
                            .clickable(role = Role.Button) {
                                flipRotationTarget += 180f
                                flipBlurActive = true
                                onFlipCamera()
                            },
                        contentAlignment = Alignment.Center,
                    ) {
                        ReiconIcon(
                            icon = Reicon.SwitchCamera,
                            contentDescription = null,
                            tint = PicooCameraColors.Content,
                            modifier = Modifier
                                .size(dimensions.iconEmphasis)
                                .rotate(flipRotation),
                        )
                    }
                }
            }
        }

        if (thermalToast) {
            Text(
                text = "设备偏热保护中，1080P 暂不可选",
                color = PicooCameraColors.WarningContent,
                style = PicooCameraTypography.Status,
                modifier = Modifier
                    .align(Alignment.TopCenter)
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

        if (reconnecting) {
            ReconnectOverlay(
                networkUnstable = networkUnstable,
                reconnectAttempt = reconnectAttempt,
                reconnectDelayMs = reconnectDelayMs,
                onStopReconnect = onStopReconnect,
            )
        }
    }
}

@Composable
private fun FocusRing(center: Offset) {
    val dimensions = PicooTheme.dimensions
    val shrink by animateFloatAsState(
        targetValue = 0.82f,
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
private fun SafeFrameOverlay() {
    val dimensions = PicooTheme.dimensions
    BoxWithConstraints(modifier = Modifier.fillMaxSize()) {
        val frameWidth = maxWidth * PicooCameraDimensions.SafeFrameWidthFraction
        val frameHeight = frameWidth / PicooCameraDimensions.VideoAspectRatio
        Box(
            modifier = Modifier
                .width(frameWidth)
                .height(frameHeight)
                .align(Alignment.Center)
                .border(
                    width = PicooCameraDimensions.BorderEmphasis,
                    color = PicooCameraColors.SafeFrame,
                    shape = RoundedCornerShape(dimensions.radiusControl),
                ),
        )
        Text(
            text = "电脑端画面 16:9 裁切框",
            color = PicooCameraColors.ContentSubtle,
            style = PicooCameraTypography.Micro,
            modifier = Modifier
                .align(Alignment.Center)
                .offset(y = (-frameHeight / 2) - dimensions.space12),
        )
    }
}

@Composable
private fun ReconnectOverlay(
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
                small = true,
                context = PicooVisualContext.Camera,
            )
        }
    }
}

@Composable
private fun EvPanel(
    exposureEv: Int,
    onEvMinus: () -> Unit,
    onEvPlus: () -> Unit,
    onEvReset: () -> Unit,
) {
    val dimensions = PicooTheme.dimensions
    val label = when (exposureEv) {
        0 -> "自动"
        1 -> "提亮 +1"
        2 -> "提亮 +2"
        -1 -> "压暗 -1"
        -2 -> "压暗 -2"
        else -> "EV $exposureEv"
    }
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(dimensions.radiusControl))
            .background(PicooCameraColors.PanelOverlay)
            .border(
                PicooCameraDimensions.Border,
                PicooCameraColors.ControlBorder,
                RoundedCornerShape(dimensions.radiusControl),
            )
            .padding(horizontal = dimensions.space12, vertical = dimensions.space8),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        PicooIconButton(
            onClick = onEvMinus,
            contentDescription = "降低曝光",
            context = PicooVisualContext.Camera,
        ) {
            Text(text = "−", color = PicooCameraColors.Content, style = PicooCameraTypography.Action)
        }
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(dimensions.space4),
        ) {
            ReiconIcon(
                icon = Reicon.Exposure,
                contentDescription = null,
                tint = PicooCameraColors.Selected,
                modifier = Modifier.size(dimensions.iconCompact),
            )
            Text(
                text = label,
                color = PicooCameraColors.Content,
                style = PicooCameraTypography.Status.copy(fontFamily = PicooFont.Mono),
            )
        }
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(dimensions.space4),
        ) {
            PicooIconButton(
                onClick = onEvPlus,
                contentDescription = "提高曝光",
                context = PicooVisualContext.Camera,
            ) {
                Text(text = "＋", color = PicooCameraColors.Content, style = PicooCameraTypography.Action)
            }
            PicooIconButton(
                onClick = onEvReset,
                contentDescription = "恢复自动测光",
                context = PicooVisualContext.Camera,
            ) {
                ReiconIcon(
                    icon = Reicon.ResetExposure,
                    contentDescription = null,
                    tint = if (exposureEv == 0) PicooCameraColors.ContentMuted else PicooCameraColors.Selected,
                    modifier = Modifier.size(PicooCameraDimensions.ToolIcon),
                )
            }
        }
    }
}

@Composable
private fun HudBadge(content: @Composable RowScope.() -> Unit) {
    val dimensions = PicooTheme.dimensions
    Row(
        modifier = Modifier
            .clip(RoundedCornerShape(PicooCameraDimensions.PillRadius))
            .background(PicooCameraColors.HudOverlay)
            .border(
                PicooCameraDimensions.Border,
                PicooCameraColors.HudBorder,
                RoundedCornerShape(PicooCameraDimensions.PillRadius),
            )
            .padding(horizontal = dimensions.space8, vertical = dimensions.space4),
        horizontalArrangement = Arrangement.spacedBy(dimensions.space4),
        verticalAlignment = Alignment.CenterVertically,
        content = content,
    )
}

@Composable
private fun CamToolButton(
    onClick: () -> Unit,
    contentDescription: String,
    enabled: Boolean = true,
    active: Boolean = false,
    content: @Composable () -> Unit,
) {
    Box(
        modifier = Modifier
            .size(PicooTheme.dimensions.touchTarget)
            .clip(CircleShape)
            .background(
                if (active) PicooCameraColors.SelectedSurface
                else PicooCameraColors.Surface.copy(alpha = 0f),
            )
            .semantics {
                this.contentDescription = contentDescription
                selected = active
                stateDescription = if (active) "已开启" else "已关闭"
                role = Role.Button
            }
            .clickable(enabled = enabled, role = Role.Button, onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        content()
    }
}

@Composable
private fun ResPill(text: String, throttled: Boolean, onClick: () -> Unit) {
    val dimensions = PicooTheme.dimensions
    // AC-M-LIVE-02: thermal warn styling (no emoji — anti-pattern §5).
    val border = if (throttled) PicooCameraColors.WarningBorderStrong else PicooCameraColors.ControlBorder
    val bg = if (throttled) PicooCameraColors.WarningSurfaceStrong else PicooCameraColors.Overlay
    val fg = if (throttled) PicooCameraColors.Warning else PicooCameraColors.Content
    val label = if (throttled) "热降档 · $text" else text
    Row(
        modifier = Modifier
            .defaultMinSize(minHeight = PicooTheme.dimensions.touchTarget)
            .clip(RoundedCornerShape(PicooCameraDimensions.PillRadius))
            .background(bg)
            .border(
                PicooCameraDimensions.Border,
                border,
                RoundedCornerShape(PicooCameraDimensions.PillRadius),
            )
            .semantics {
                contentDescription = "切换画质，当前 $label"
                role = Role.Button
            }
            .clickable(role = Role.Button, onClick = onClick)
            .padding(horizontal = dimensions.space8, vertical = dimensions.space4),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(dimensions.space4),
    ) {
        if (throttled) {
            ReiconIcon(
                icon = Reicon.Overheat,
                contentDescription = "过热降档",
                tint = PicooCameraColors.Warning,
                modifier = Modifier.size(PicooCameraDimensions.ThermalIcon),
            )
        }
        Text(
            text = label,
            color = fg,
            style = PicooCameraTypography.Label.copy(
                fontFamily = PicooFont.Mono,
                fontWeight = FontWeight.Bold,
            ),
        )
    }
}

@Composable
private fun StatPill(bitrate: String, packetLossLabel: String) {
    val dimensions = PicooTheme.dimensions
    Column(
        modifier = Modifier
            .clip(RoundedCornerShape(dimensions.radiusControl))
            .background(PicooCameraColors.StatSurface)
            .border(
                PicooCameraDimensions.Border,
                PicooCameraColors.ControlBorderSubtle,
                RoundedCornerShape(dimensions.radiusControl),
            )
            .padding(horizontal = dimensions.space8, vertical = dimensions.space4),
        horizontalAlignment = Alignment.Start,
    ) {
        Text(
            text = bitrate,
            color = PicooCameraColors.Content,
            style = PicooCameraTypography.Status.copy(
                fontFamily = PicooFont.Mono,
                fontWeight = FontWeight.Bold,
            ),
        )
        Text(
            text = "30 FPS · $packetLossLabel",
            color = PicooCameraColors.Success,
            style = PicooCameraTypography.Micro.copy(fontFamily = PicooFont.Mono),
        )
    }
}

@Composable
private fun ThermalBanner() {
    val dimensions = PicooTheme.dimensions
    Text(
        text = "设备偏热保护：已自动降至 720P 稳住帧率，1080P 暂不可选",
        color = PicooCameraColors.WarningContent,
        style = PicooCameraTypography.Status,
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
