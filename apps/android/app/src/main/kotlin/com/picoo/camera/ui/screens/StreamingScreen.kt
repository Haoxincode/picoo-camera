package com.picoo.camera.ui.screens

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
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.BrightnessHigh
import androidx.compose.material.icons.filled.Cameraswitch
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.LockOpen
import androidx.compose.material.icons.filled.Sync
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
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
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlin.math.roundToInt
import kotlinx.coroutines.delay
import com.picoo.camera.media.LensFacing
import com.picoo.camera.ui.CameraPreviewSurface
import com.picoo.camera.ui.components.PicooGhostButton
import com.picoo.camera.ui.components.PicooPrimaryButton
import com.picoo.camera.ui.ReconnectBackoffFormat
import com.picoo.camera.ui.theme.PicooColors
import com.picoo.camera.ui.theme.PicooFont

/** REQ-PICOO-UI-003 / REQ-PICOO-UI-005 — 传输页，对齐 m-screen-streaming。 */
@Composable
fun StreamingScreen(
    cameraGranted: Boolean,
    receiverName: String,
    linkQualityChip: String,
    resolutionLabel: String,
    bitrateMbps: String,
    lensFacing: LensFacing,
    localPreviewMirrored: Boolean,
    thermalForced720: Boolean,
    powerHint: String,
    reconnecting: Boolean,
    networkUnstable: Boolean = false,
    reconnectAttempt: Int = 0,
    reconnectDelayMs: Long = 0L,
    packetLossLabel: String,
    onRequestCamera: () -> Unit,
    onFlipCamera: () -> Unit,
    onToggleResolution: () -> Unit,
    onToggleMirror: () -> Unit,
    onEvStep: () -> Unit,
    exposureEv: Int,
    evSupported: Boolean,
    onDisconnect: () -> Unit,
    onStopReconnect: () -> Unit,
    onPreviewSurfaceAvailable: (android.view.Surface) -> Unit,
    onPreviewSurfaceDestroyed: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var uiLocked by remember { mutableStateOf(false) }
    var showEvPanel by remember { mutableStateOf(false) }
    var shutterArmed by remember { mutableStateOf(false) }
    var flipRotationTarget by remember { mutableFloatStateOf(0f) }
    var immersive by remember { mutableStateOf(false) }
    var focusRingCenter by remember { mutableStateOf(Offset.Zero) }
    var focusRingActive by remember { mutableStateOf(false) }
    val flipRotation by animateFloatAsState(
        targetValue = flipRotationTarget,
        animationSpec = tween(durationMillis = 280),
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

    LaunchedEffect(uiLocked) {
        if (uiLocked) {
            immersive = false
        }
    }

    val showChrome = !uiLocked && !immersive

    Box(
        modifier = modifier
            .fillMaxSize()
            .background(PicooColors.LiveBg),
    ) {
        if (cameraGranted) {
            CameraPreviewSurface(
                modifier = Modifier.fillMaxSize(),
                mirrorLocal = localPreviewMirrored,
                onSurfaceAvailable = onPreviewSurfaceAvailable,
                onSurfaceDestroyed = onPreviewSurfaceDestroyed,
            )
        } else {
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .background(PicooColors.Panel),
                contentAlignment = Alignment.Center,
            ) {
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Text(
                        text = "需要相机权限才能预览与推流",
                        color = PicooColors.Muted,
                        fontSize = 14.sp,
                    )
                    Spacer(modifier = Modifier.height(12.dp))
                    PicooPrimaryButton(
                        text = "启用相机",
                        onClick = onRequestCamera,
                        modifier = Modifier.padding(horizontal = 32.dp),
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
                    .padding(start = 12.dp, end = 12.dp, top = 40.dp),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                HudBadge {
                    Box(
                        modifier = Modifier
                            .size(6.dp)
                            .background(PicooColors.Ready, CircleShape),
                    )
                    Text(
                        text = receiverName.ifBlank { "电脑" },
                        color = PicooColors.Text,
                        fontSize = 12.sp,
                        fontWeight = FontWeight.SemiBold,
                    )
                    val latency = linkQualityChip.substringAfter(" · ", "")
                    if (latency.isNotEmpty()) {
                        Text(
                            text = latency,
                            color = PicooColors.Ready,
                            fontSize = 11.sp,
                            fontFamily = PicooFont.Mono,
                        )
                    }
                }

                Row(
                    modifier = Modifier
                        .clip(RoundedCornerShape(999.dp))
                        .background(Color(0x8C0A0C10))
                        .border(1.dp, Color(0x1FFFFFFF), RoundedCornerShape(999.dp))
                        .padding(horizontal = 4.dp, vertical = 3.dp),
                    horizontalArrangement = Arrangement.spacedBy(2.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    CamToolButton(
                        onClick = { showEvPanel = !showEvPanel },
                        enabled = evSupported,
                        active = showEvPanel,
                        contentDescription = "曝光补偿",
                    ) {
                        Icon(
                            imageVector = Icons.Default.BrightnessHigh,
                            contentDescription = null,
                            tint = if (showEvPanel) PicooColors.Accent2 else PicooColors.Muted,
                            modifier = Modifier.size(15.dp),
                        )
                    }
                    CamToolButton(
                        onClick = onToggleMirror,
                        active = localPreviewMirrored,
                        contentDescription = "预览镜像",
                    ) {
                        Icon(
                            imageVector = Icons.Default.Sync,
                            contentDescription = null,
                            tint = if (localPreviewMirrored) PicooColors.Accent2 else PicooColors.Muted,
                            modifier = Modifier.size(15.dp),
                        )
                    }
                    CamToolButton(
                        onClick = { uiLocked = !uiLocked },
                        active = uiLocked,
                        contentDescription = "防误触锁定",
                    ) {
                        Icon(
                            imageVector = if (uiLocked) Icons.Default.Lock else Icons.Default.LockOpen,
                            contentDescription = null,
                            tint = if (uiLocked) PicooColors.Accent2 else PicooColors.Muted,
                            modifier = Modifier.size(15.dp),
                        )
                    }
                }

                ResPill(
                    text = "${resolutionLabel.uppercase()} · 30",
                    throttled = thermalForced720,
                    onClick = onToggleResolution,
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
                            colors = listOf(Color.Transparent, Color(0xF2030406)),
                        ),
                    )
                    .padding(start = 18.dp, end = 18.dp, top = 10.dp, bottom = 24.dp),
            ) {
                if (thermalForced720) {
                    ThermalBanner()
                    Spacer(modifier = Modifier.height(8.dp))
                } else if (powerHint.isNotEmpty()) {
                    Text(
                        text = powerHint,
                        color = PicooColors.Warn,
                        fontSize = 13.sp,
                        modifier = Modifier
                            .fillMaxWidth()
                            .clip(RoundedCornerShape(12.dp))
                            .background(Color(0x24F0C14A))
                            .border(1.dp, Color(0x47F0C14A), RoundedCornerShape(12.dp))
                            .padding(10.dp),
                    )
                    Spacer(modifier = Modifier.height(8.dp))
                }

                if (showEvPanel && evSupported) {
                    EvPanel(exposureEv = exposureEv, onEvStep = onEvStep)
                    Spacer(modifier = Modifier.height(8.dp))
                }

                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    StatPill(bitrate = bitrateMbps, packetLossLabel = packetLossLabel)

                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        Box(
                            modifier = Modifier
                                .size(64.dp)
                                .clip(CircleShape)
                                .border(
                                    width = 3.5.dp,
                                    color = if (shutterArmed) PicooColors.Danger else Color.White.copy(alpha = 0.85f),
                                    shape = CircleShape,
                                )
                                .background(Color(0x4D000000))
                                .clickable {
                                    if (shutterArmed) {
                                        onDisconnect()
                                    } else {
                                        shutterArmed = true
                                    }
                                },
                            contentAlignment = Alignment.Center,
                        ) {
                            Box(
                                modifier = Modifier
                                    .size(if (shutterArmed) 26.dp else 22.dp)
                                    .clip(if (shutterArmed) CircleShape else RoundedCornerShape(5.dp))
                                    .background(if (shutterArmed) Color(0xFFFF2D46) else PicooColors.Danger),
                            )
                        }
                        Spacer(modifier = Modifier.height(4.dp))
                        Text(
                            text = if (shutterArmed) "再次点击确认断开" else "断开连接",
                            color = if (shutterArmed) PicooColors.DangerText else PicooColors.Muted,
                            fontSize = 11.sp,
                            fontWeight = if (shutterArmed) FontWeight.Bold else FontWeight.SemiBold,
                        )
                    }

                    Box(
                        modifier = Modifier
                            .size(46.dp)
                            .clip(CircleShape)
                            .border(1.dp, Color(0x2FFFFFFF), CircleShape)
                            .background(Color(0x14FFFFFF))
                            .clickable {
                                flipRotationTarget += 180f
                                onFlipCamera()
                            },
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(
                            imageVector = Icons.Default.Cameraswitch,
                            contentDescription = "切换前后摄像头",
                            tint = Color.White,
                            modifier = Modifier
                                .size(22.dp)
                                .rotate(flipRotation),
                        )
                    }
                }
            }
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
    val halfPx = with(LocalDensity.current) { 28.dp.toPx() }
    Box(
        modifier = Modifier
            .offset {
                IntOffset(
                    (center.x - halfPx).roundToInt(),
                    (center.y - halfPx).roundToInt(),
                )
            }
            .size(56.dp)
            .border(1.5.dp, Color(0xFFFFDC52), RoundedCornerShape(8.dp)),
    )
}

@Composable
private fun SafeFrameOverlay() {
    BoxWithConstraints(modifier = Modifier.fillMaxSize()) {
        val frameWidth = maxWidth * 0.9f
        val frameHeight = frameWidth * 9f / 16f
        Box(
            modifier = Modifier
                .width(frameWidth)
                .height(frameHeight)
                .align(Alignment.Center)
                .border(
                    width = 1.5.dp,
                    color = Color(0x59FFFFFF),
                    shape = RoundedCornerShape(8.dp),
                ),
        )
        Text(
            text = "电脑端画面 16:9 裁切框",
            color = Color(0x8CFFFFFF),
            fontSize = 10.sp,
            letterSpacing = 0.5.sp,
            modifier = Modifier
                .align(Alignment.Center)
                .offset(y = (-frameHeight / 2) - 12.dp),
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
    val title = if (networkUnstable) "网络不稳定，正在优化…" else "网络中断，正在重连…"
    val detail = if (networkUnstable) {
        "链路丢包较高，保持推流并等待恢复\n连接恢复后将自动请求 IDR 关键帧"
    } else {
        ReconnectBackoffFormat.detailMessage(reconnectAttempt, reconnectDelayMs)
    }
    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(Color(0xD9050608)),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            modifier = Modifier.padding(24.dp),
        ) {
            CircularProgressIndicator(
                color = PicooColors.Accent,
                strokeWidth = 3.dp,
                modifier = Modifier.size(40.dp),
            )
            Spacer(modifier = Modifier.height(16.dp))
            Text(
                text = title,
                color = PicooColors.Text,
                fontFamily = PicooFont.Display,
                fontSize = 18.sp,
                fontWeight = FontWeight.Bold,
            )
            Spacer(modifier = Modifier.height(8.dp))
            Text(
                text = detail,
                color = PicooColors.Muted,
                fontSize = 13.sp,
                lineHeight = 20.sp,
                textAlign = TextAlign.Center,
            )
            Spacer(modifier = Modifier.height(16.dp))
            PicooGhostButton(text = "停止重连并退出", onClick = onStopReconnect, small = true)
        }
    }
}

@Composable
private fun EvPanel(exposureEv: Int, onEvStep: () -> Unit) {
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
            .clip(RoundedCornerShape(12.dp))
            .background(Color(0x660A0C10))
            .border(1.dp, PicooColors.Line, RoundedCornerShape(12.dp))
            .padding(horizontal = 12.dp, vertical = 8.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(text = "−", color = PicooColors.Text, modifier = Modifier.clickable { onEvStep() })
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
            Icon(
                imageVector = Icons.Default.BrightnessHigh,
                contentDescription = null,
                tint = PicooColors.Accent2,
                modifier = Modifier.size(16.dp),
            )
            Text(
                text = label,
                color = PicooColors.Text,
                fontSize = 13.sp,
                fontFamily = PicooFont.Mono,
            )
        }
        Text(text = "＋", color = PicooColors.Text, modifier = Modifier.clickable { onEvStep() })
    }
}

@Composable
private fun HudBadge(content: @Composable RowScope.() -> Unit) {
    Row(
        modifier = Modifier
            .clip(RoundedCornerShape(999.dp))
            .background(Color(0xA60A0C10))
            .border(1.dp, Color(0x24FFFFFF), RoundedCornerShape(999.dp))
            .padding(horizontal = 10.dp, vertical = 5.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
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
            .size(28.dp)
            .clip(CircleShape)
            .background(if (active) Color(0x33FFB347) else Color.Transparent)
            .clickable(enabled = enabled, onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        content()
    }
}

@Composable
private fun ResPill(text: String, throttled: Boolean, onClick: () -> Unit) {
    val border = if (throttled) Color(0x80F0C14A) else Color(0x38FFFFFF)
    val bg = if (throttled) Color(0x33F0C14A) else Color(0xB30A0C10)
    val fg = if (throttled) PicooColors.Warn else Color.White
    Text(
        text = text,
        modifier = Modifier
            .clip(RoundedCornerShape(999.dp))
            .background(bg)
            .border(1.dp, border, RoundedCornerShape(999.dp))
            .clickable(onClick = onClick)
            .padding(horizontal = 10.dp, vertical = 5.dp),
        color = fg,
        fontSize = 11.sp,
        fontWeight = FontWeight.Bold,
        fontFamily = PicooFont.Mono,
    )
}

@Composable
private fun StatPill(bitrate: String, packetLossLabel: String) {
    Column(
        modifier = Modifier
            .clip(RoundedCornerShape(12.dp))
            .background(Color(0x0FFFFFFF))
            .border(1.dp, Color(0x1AFFFFFF), RoundedCornerShape(12.dp))
            .padding(horizontal = 10.dp, vertical = 6.dp),
        horizontalAlignment = Alignment.Start,
    ) {
        Text(
            text = bitrate,
            color = Color.White,
            fontSize = 13.sp,
            fontWeight = FontWeight.Bold,
            fontFamily = PicooFont.Mono,
        )
        Text(
            text = "30 FPS · $packetLossLabel",
            color = PicooColors.Ready,
            fontSize = 10.sp,
            fontFamily = PicooFont.Mono,
        )
    }
}

@Composable
private fun ThermalBanner() {
    Text(
        text = "设备偏热保护：已自动降至 720P 稳住帧率，1080P 暂不可选",
        color = Color(0xFFFFE6A4),
        fontSize = 13.sp,
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(12.dp))
            .background(Color(0x24F0C14A))
            .border(1.dp, Color(0x47F0C14A), RoundedCornerShape(12.dp))
            .padding(10.dp),
    )
}
