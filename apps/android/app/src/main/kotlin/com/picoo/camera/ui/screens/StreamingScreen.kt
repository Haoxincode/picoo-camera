package com.picoo.camera.ui.screens

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
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
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.blur
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.draw.scale
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.disabled
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.IntOffset
import com.picoo.camera.ui.CameraPreviewSurface
import com.picoo.camera.ui.ExposurePreview
import com.picoo.camera.ui.ReconnectBackoffFormat
import com.picoo.camera.ui.components.PicooButtonSize
import com.picoo.camera.ui.components.PicooGhostButton
import com.picoo.camera.ui.components.PicooPrimaryButton
import com.picoo.camera.ui.components.PicooVisualContext
import com.picoo.camera.ui.components.Reicon
import com.picoo.camera.ui.components.ReiconIcon
import com.picoo.camera.ui.theme.PicooCameraColors
import com.picoo.camera.ui.theme.PicooCameraDimensions
import com.picoo.camera.ui.theme.PicooCameraTypography
import com.picoo.camera.ui.theme.PicooFont
import com.picoo.camera.ui.theme.PicooTheme
import kotlin.math.roundToInt
import kotlinx.coroutines.delay

/** REQ-PICOO-UI-003 / REQ-PICOO-UI-012 — 连接后的沉浸式相机控制台。 */
@Composable
fun StreamingScreen(
    cameraGranted: Boolean,
    cameraPermissionPermanentlyDenied: Boolean,
    receiverName: String,
    linkQualityChip: String,
    resolutionLabel: String,
    bitrateMbps: String,
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
    onCycleExposure: () -> Unit,
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
    StreamingScreenContent(
        cameraGranted = cameraGranted,
        cameraPermissionPermanentlyDenied = cameraPermissionPermanentlyDenied,
        receiverName = receiverName,
        linkQualityChip = linkQualityChip,
        resolutionLabel = resolutionLabel,
        bitrateMbps = bitrateMbps,
        localPreviewMirrored = localPreviewMirrored,
        thermalForced720 = thermalForced720,
        powerHint = powerHint,
        reconnecting = reconnecting,
        packetLossLabel = packetLossLabel,
        onRequestCamera = onRequestCamera,
        onFlipCamera = onFlipCamera,
        onToggleResolution = onToggleResolution,
        onToggleMirror = onToggleMirror,
        onCycleExposure = onCycleExposure,
        exposureEv = exposureEv,
        evSupported = evSupported,
        onDisconnect = onDisconnect,
        onStopReconnect = onStopReconnect,
        modifier = modifier,
        networkUnstable = networkUnstable,
        reconnectAttempt = reconnectAttempt,
        reconnectDelayMs = reconnectDelayMs,
        previewContent = {
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
        },
    )
}

/** Plain rendering boundary; the caller supplies the real or deterministic preview surface. */
@Composable
internal fun StreamingScreenContent(
    cameraGranted: Boolean,
    cameraPermissionPermanentlyDenied: Boolean,
    receiverName: String,
    linkQualityChip: String,
    resolutionLabel: String,
    bitrateMbps: String,
    localPreviewMirrored: Boolean,
    thermalForced720: Boolean,
    powerHint: String,
    reconnecting: Boolean,
    packetLossLabel: String,
    onRequestCamera: () -> Unit,
    onFlipCamera: () -> Unit,
    onToggleResolution: () -> Unit,
    onToggleMirror: () -> Unit,
    onCycleExposure: () -> Unit,
    exposureEv: Int,
    evSupported: Boolean,
    onDisconnect: () -> Unit,
    onStopReconnect: () -> Unit,
    previewContent: @Composable () -> Unit,
    modifier: Modifier = Modifier,
    networkUnstable: Boolean = false,
    reconnectAttempt: Int = 0,
    reconnectDelayMs: Long = 0L,
) {
    val motion = PicooTheme.motion
    var uiLocked by remember { mutableStateOf(false) }
    var disconnectArmed by remember { mutableStateOf(false) }
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

    LaunchedEffect(disconnectArmed) {
        if (disconnectArmed) {
            delay(PicooCameraDimensions.DisconnectConfirmMillis)
            disconnectArmed = false
        }
    }
    LaunchedEffect(focusRingActive) {
        if (focusRingActive) {
            delay(PicooCameraDimensions.FocusRingVisibleMillis)
            focusRingActive = false
        }
    }
    LaunchedEffect(flipBlurActive) {
        if (flipBlurActive) {
            delay(PicooCameraDimensions.FlipBlurMillis)
            flipBlurActive = false
        }
    }
    LaunchedEffect(thermalToast) {
        if (thermalToast) {
            delay(PicooCameraDimensions.ToastVisibleMillis)
            thermalToast = false
        }
    }

    Box(
        modifier = modifier
            .fillMaxSize()
            .background(PicooCameraColors.Surface),
    ) {
        if (cameraGranted) {
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .semantics {
                        contentDescription = "本机相机预览"
                        stateDescription = "全屏预览，电脑端固定输出中央 16:9"
                    },
            ) {
                previewContent()
                val overlayAlpha = ExposurePreview.overlayAlpha(exposureEv)
                if (overlayAlpha > 0f) {
                    Box(
                        modifier = Modifier
                            .fillMaxSize()
                            .background(
                                ExposurePreview.overlayColor(exposureEv).copy(alpha = overlayAlpha),
                            ),
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
                if (!uiLocked) {
                    Box(
                        modifier = Modifier
                            .fillMaxSize()
                            .pointerInput(immersive) {
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
                if (focusRingActive) FocusRing(center = focusRingCenter)
            }
        } else {
            CameraPermissionPlaceholder(
                permanentlyDenied = cameraPermissionPermanentlyDenied,
                onRequestCamera = onRequestCamera,
            )
        }

        if (!immersive) {
            ConnectionHud(
                receiverName = receiverName,
                linkQualityChip = linkQualityChip,
                bitrateMbps = bitrateMbps,
                resolutionLabel = resolutionLabel,
                packetLossLabel = packetLossLabel,
                thermalForced720 = thermalForced720,
                enabled = !uiLocked,
                onToggleResolution = {
                    if (thermalForced720 && resolutionLabel.contains("720", ignoreCase = true)) {
                        thermalToast = true
                    } else {
                        onToggleResolution()
                    }
                },
                modifier = Modifier.align(Alignment.TopCenter),
            )
            CameraControlDock(
                exposureEv = exposureEv,
                evSupported = evSupported,
                localPreviewMirrored = localPreviewMirrored,
                uiLocked = uiLocked,
                disconnectArmed = disconnectArmed,
                flipRotation = flipRotation,
                thermalForced720 = thermalForced720,
                powerHint = powerHint,
                onCycleExposure = onCycleExposure,
                onToggleMirror = onToggleMirror,
                onToggleLock = {
                    uiLocked = !uiLocked
                    disconnectArmed = false
                },
                onDisconnect = {
                    if (disconnectArmed) onDisconnect() else disconnectArmed = true
                },
                onFlipCamera = {
                    flipRotationTarget += 180f
                    flipBlurActive = true
                    onFlipCamera()
                },
                modifier = Modifier.align(Alignment.BottomCenter),
            )
        }

        if (thermalToast) {
            CameraToast(
                text = "设备偏热保护中，1080P 暂不可选",
                modifier = Modifier.align(Alignment.TopCenter),
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
private fun CameraPermissionPlaceholder(
    permanentlyDenied: Boolean,
    onRequestCamera: () -> Unit,
) {
    val dimensions = PicooTheme.dimensions
    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(PicooCameraColors.SurfaceRaised),
        contentAlignment = Alignment.Center,
    ) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            Text(
                text = if (permanentlyDenied) {
                    "相机权限已关闭，请前往系统设置开启后继续推流"
                } else {
                    "需要相机权限才能预览与推流"
                },
                color = PicooCameraColors.ContentMuted,
                style = PicooCameraTypography.Status,
                textAlign = TextAlign.Center,
                modifier = Modifier.padding(horizontal = dimensions.space32),
            )
            Spacer(modifier = Modifier.height(dimensions.space12))
            PicooPrimaryButton(
                text = if (permanentlyDenied) "前往设置" else "启用相机",
                onClick = onRequestCamera,
                modifier = Modifier.padding(horizontal = dimensions.space32),
                context = PicooVisualContext.Camera,
            )
        }
    }
}

@Composable
private fun ConnectionHud(
    receiverName: String,
    linkQualityChip: String,
    bitrateMbps: String,
    resolutionLabel: String,
    packetLossLabel: String,
    thermalForced720: Boolean,
    enabled: Boolean,
    onToggleResolution: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val dimensions = PicooTheme.dimensions
    val latency = linkQualityChip.substringAfter(" · ", missingDelimiterValue = "--ms")
    val displayName = receiverName.ifBlank { "Picoo Camera" }
    Column(
        modifier = modifier
            .statusBarsPadding()
            .widthIn(max = dimensions.controlContentWidth)
            .fillMaxWidth()
            .padding(
                start = dimensions.space24,
                end = dimensions.space24,
                top = dimensions.space16,
            ),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Row(
            modifier = Modifier.semantics(mergeDescendants = true) {
                contentDescription = "$displayName 已连接"
                stateDescription = "$latency，$bitrateMbps，$packetLossLabel"
            },
            horizontalArrangement = Arrangement.spacedBy(dimensions.space8),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(
                modifier = Modifier
                    .size(PicooCameraDimensions.ConnectionDot)
                    .background(PicooCameraColors.Selected, CircleShape),
            )
            Text(
                text = "$displayName 已连接",
                color = PicooCameraColors.Content,
                style = PicooCameraTypography.HudTitle,
            )
        }
        Spacer(modifier = Modifier.height(dimensions.space4))
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(dimensions.space8),
        ) {
            TelemetryText(text = latency)
            TelemetrySeparator()
            TelemetryText(text = bitrateMbps)
            TelemetrySeparator()
            ResolutionMetric(
                resolutionLabel = resolutionLabel,
                thermalForced720 = thermalForced720,
                enabled = enabled,
                onClick = onToggleResolution,
            )
        }
    }
}

@Composable
private fun TelemetryText(text: String) {
    Text(
        text = text,
        color = PicooCameraColors.ContentMuted,
        style = PicooCameraTypography.Telemetry.copy(fontFamily = PicooFont.Mono),
    )
}

@Composable
private fun TelemetrySeparator() {
    Text(
        text = "•",
        color = PicooCameraColors.ContentSubtle,
        style = PicooCameraTypography.Telemetry,
    )
}

@Composable
private fun ResolutionMetric(
    resolutionLabel: String,
    thermalForced720: Boolean,
    enabled: Boolean,
    onClick: () -> Unit,
) {
    val dimensions = PicooTheme.dimensions
    Row(
        modifier = Modifier
            .height(dimensions.touchTarget)
            .alpha(if (enabled) 1f else PicooCameraDimensions.DisabledAlpha)
            .semantics {
                contentDescription = "切换画质，当前 $resolutionLabel 30fps"
                stateDescription = if (thermalForced720) "设备偏热，已限制为 720p" else "可切换"
                role = Role.Button
                if (!enabled) disabled()
            }
            .clickable(enabled = enabled, role = Role.Button, onClick = onClick),
        horizontalArrangement = Arrangement.spacedBy(dimensions.space4),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (thermalForced720) {
            ReiconIcon(
                icon = Reicon.Overheat,
                contentDescription = null,
                tint = PicooCameraColors.Warning,
                modifier = Modifier.size(dimensions.iconCompact),
            )
        }
        Text(
            text = resolutionLabel.lowercase(),
            color = if (thermalForced720) PicooCameraColors.Warning else PicooCameraColors.ContentMuted,
            style = PicooCameraTypography.Telemetry.copy(fontFamily = PicooFont.Mono),
        )
        Text(
            text = "/",
            color = PicooCameraColors.Selected,
            style = PicooCameraTypography.Telemetry.copy(fontFamily = PicooFont.Mono),
        )
        Text(
            text = "30fps",
            color = PicooCameraColors.ContentMuted,
            style = PicooCameraTypography.Telemetry.copy(fontFamily = PicooFont.Mono),
        )
    }
}

@Composable
private fun CameraControlDock(
    exposureEv: Int,
    evSupported: Boolean,
    localPreviewMirrored: Boolean,
    uiLocked: Boolean,
    disconnectArmed: Boolean,
    flipRotation: Float,
    thermalForced720: Boolean,
    powerHint: String,
    onCycleExposure: () -> Unit,
    onToggleMirror: () -> Unit,
    onToggleLock: () -> Unit,
    onDisconnect: () -> Unit,
    onFlipCamera: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val dimensions = PicooTheme.dimensions
    Column(
        modifier = modifier
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
                start = dimensions.space24,
                end = dimensions.space24,
                top = dimensions.space32,
                bottom = dimensions.space16,
            ),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Column(
            modifier = Modifier
                .widthIn(max = dimensions.controlContentWidth)
                .fillMaxWidth(),
        ) {
            if (thermalForced720) {
                ThermalBanner()
                Spacer(modifier = Modifier.height(dimensions.space8))
            } else if (powerHint.isNotEmpty()) {
                PowerHint(text = powerHint)
                Spacer(modifier = Modifier.height(dimensions.space8))
            }

            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                DockControlButton(
                    icon = Reicon.Exposure,
                    label = exposureControlLabel(exposureEv),
                    contentDescription = "调节亮度",
                    stateDescription = exposureStateDescription(exposureEv, evSupported),
                    selected = exposureEv != 0,
                    enabled = !uiLocked && evSupported,
                    onClick = onCycleExposure,
                    modifier = Modifier.weight(1f),
                )
                DockDivider()
                DockControlButton(
                    icon = Reicon.Mirror,
                    label = "镜像",
                    contentDescription = "切换本机预览镜像",
                    stateDescription = if (localPreviewMirrored) "已开启" else "已关闭",
                    selected = localPreviewMirrored,
                    enabled = !uiLocked,
                    onClick = onToggleMirror,
                    modifier = Modifier.weight(1f),
                )
                DockDivider()
                DockControlButton(
                    icon = if (uiLocked) Reicon.InteractionLock else Reicon.InteractionUnlock,
                    label = if (uiLocked) "已锁定" else "锁定",
                    contentDescription = "防误触锁定",
                    stateDescription = if (uiLocked) "已锁定，再次点击解锁" else "未锁定",
                    selected = uiLocked,
                    enabled = true,
                    onClick = onToggleLock,
                    modifier = Modifier.weight(1f),
                )
            }

            Spacer(modifier = Modifier.height(dimensions.space8))
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .height(PicooCameraDimensions.DockHairline)
                    .background(PicooCameraColors.ControlBorderSubtle),
            )
            if (disconnectArmed) {
                Text(
                    text = "再次点击确认断开",
                    color = PicooCameraColors.DangerContent,
                    style = PicooCameraTypography.GuideHint,
                    textAlign = TextAlign.Center,
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(top = dimensions.space8),
                )
            }
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                DockActionButton(
                    icon = Reicon.Disconnect,
                    contentDescription = "断开连接",
                    stateDescription = if (disconnectArmed) "等待再次确认" else "未确认",
                    tint = if (disconnectArmed) {
                        PicooCameraColors.DangerEmphasis
                    } else {
                        PicooCameraColors.Danger
                    },
                    enabled = !uiLocked,
                    onClick = onDisconnect,
                    modifier = Modifier.weight(1f),
                )
                DockDivider()
                DockActionButton(
                    icon = Reicon.SwitchCamera,
                    contentDescription = "切换前后摄像头",
                    stateDescription = "可用",
                    tint = PicooCameraColors.Content,
                    enabled = !uiLocked,
                    iconRotation = flipRotation,
                    onClick = onFlipCamera,
                    modifier = Modifier.weight(1f),
                )
            }
        }
    }
}

@Composable
private fun DockControlButton(
    icon: Reicon,
    label: String,
    contentDescription: String,
    stateDescription: String,
    selected: Boolean,
    enabled: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val dimensions = PicooTheme.dimensions
    val tint = if (selected) PicooCameraColors.Selected else PicooCameraColors.Content
    Column(
        modifier = modifier
            .height(PicooCameraDimensions.DockControlHeight)
            .alpha(if (enabled) 1f else PicooCameraDimensions.DisabledAlpha)
            .semantics {
                this.contentDescription = contentDescription
                this.stateDescription = stateDescription
                this.selected = selected
                role = Role.Button
                if (!enabled) disabled()
            }
            .clickable(enabled = enabled, role = Role.Button, onClick = onClick),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        ReiconIcon(
            icon = icon,
            contentDescription = null,
            tint = tint,
            modifier = Modifier.size(dimensions.iconEmphasis),
        )
        Spacer(modifier = Modifier.height(dimensions.space4))
        Text(
            text = label,
            color = tint,
            style = PicooCameraTypography.ControlLabel,
            maxLines = 1,
        )
    }
}

@Composable
private fun DockActionButton(
    icon: Reicon,
    contentDescription: String,
    stateDescription: String,
    tint: Color,
    enabled: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    iconRotation: Float = 0f,
) {
    val dimensions = PicooTheme.dimensions
    Box(
        modifier = modifier
            .height(PicooCameraDimensions.DockActionHeight)
            .alpha(if (enabled) 1f else PicooCameraDimensions.DisabledAlpha)
            .semantics {
                this.contentDescription = contentDescription
                this.stateDescription = stateDescription
                role = Role.Button
                if (!enabled) disabled()
            }
            .clickable(enabled = enabled, role = Role.Button, onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        ReiconIcon(
            icon = icon,
            contentDescription = null,
            tint = tint,
            modifier = Modifier
                .size(dimensions.iconEmphasis)
                .rotate(iconRotation),
        )
    }
}

@Composable
private fun DockDivider() {
    Box(
        modifier = Modifier
            .width(PicooCameraDimensions.DockHairline)
            .height(PicooCameraDimensions.DockDividerHeight)
            .background(PicooCameraColors.ControlBorder),
    )
}

private fun exposureControlLabel(exposureEv: Int): String = when (exposureEv) {
    0 -> "亮度"
    1 -> "提亮 +1"
    2 -> "提亮 +2"
    -1 -> "压暗 -1"
    -2 -> "压暗 -2"
    else -> "EV $exposureEv"
}

private fun exposureStateDescription(exposureEv: Int, supported: Boolean): String = when {
    !supported -> "当前设备不支持曝光补偿"
    exposureEv == 0 -> "自动测光"
    else -> exposureControlLabel(exposureEv)
}

@Composable
private fun PowerHint(text: String) {
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
private fun ThermalBanner() {
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
private fun CameraToast(text: String, modifier: Modifier = Modifier) {
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
private fun FocusRing(center: Offset) {
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
                size = PicooButtonSize.Compact,
                context = PicooVisualContext.Camera,
            )
        }
    }
}
