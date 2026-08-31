package com.picoo.camera.ui.screens

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
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
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.material3.Text
import com.picoo.camera.ui.CameraPreviewSurface
import com.picoo.camera.ui.ExposurePreview
import com.picoo.camera.ui.components.PicooPrimaryButton
import com.picoo.camera.ui.components.PicooVisualContext
import com.picoo.camera.ui.theme.PicooCameraColors
import com.picoo.camera.ui.theme.PicooCameraDimensions
import com.picoo.camera.ui.theme.PicooCameraTypography
import com.picoo.camera.ui.theme.PicooTheme
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
internal fun CameraPermissionPlaceholder(
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
