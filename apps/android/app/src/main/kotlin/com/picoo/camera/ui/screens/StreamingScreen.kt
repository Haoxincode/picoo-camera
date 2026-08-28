package com.picoo.camera.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.BrightnessHigh
import androidx.compose.material.icons.filled.Cameraswitch
import androidx.compose.material.icons.filled.Sync
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.picoo.camera.media.LensFacing
import com.picoo.camera.ui.CameraPreviewSurface
import com.picoo.camera.ui.components.PicooPrimaryButton
import com.picoo.camera.ui.theme.PicooColors

/** REQ-PICOO-UI-003 / REQ-PICOO-UI-005 — 传输页：原生相机 HUD + 快门断开 + 翻转。 */
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
    onRequestCamera: () -> Unit,
    onFlipCamera: () -> Unit,
    onToggleResolution: () -> Unit,
    onToggleMirror: () -> Unit,
    onEvStep: () -> Unit,
    exposureEv: Int,
    evSupported: Boolean,
    onDisconnect: () -> Unit,
    onPreviewSurfaceAvailable: (android.view.Surface) -> Unit,
    onPreviewSurfaceDestroyed: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Box(
        modifier = modifier
            .fillMaxSize()
            .background(Color(0xFF050607)),
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

        // Top HUD
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(start = 16.dp, end = 16.dp, top = 44.dp),
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
                    color = Color(0xFFD7DBE6),
                    fontSize = 13.sp,
                    fontWeight = FontWeight.SemiBold,
                )
                if (linkQualityChip.isNotEmpty()) {
                    Text(
                        text = linkQualityChip.substringAfter(" · ", linkQualityChip),
                        color = PicooColors.Ready,
                        fontSize = 12.sp,
                        fontFamily = FontFamily.Monospace,
                    )
                }
            }

            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                CamToolButton(
                    onClick = onEvStep,
                    enabled = evSupported,
                    contentDescription = "曝光补偿",
                ) {
                    Icon(
                        imageVector = Icons.Default.BrightnessHigh,
                        contentDescription = null,
                        tint = if (evSupported) PicooColors.Text else PicooColors.Muted,
                        modifier = Modifier.size(18.dp),
                    )
                }
                CamToolButton(
                    onClick = onToggleMirror,
                    contentDescription = "预览镜像",
                ) {
                    Icon(
                        imageVector = Icons.Default.Sync,
                        contentDescription = null,
                        tint = if (localPreviewMirrored) PicooColors.Accent else PicooColors.Text,
                        modifier = Modifier.size(18.dp),
                    )
                }
            }

            ResPill(
                text = "${resolutionLabel.uppercase()} · 30",
                onClick = onToggleResolution,
            )
        }

        // Bottom native camera deck
        Column(
            modifier = Modifier
                .align(Alignment.BottomCenter)
                .fillMaxWidth()
                .background(
                    Brush.verticalGradient(
                        colors = listOf(Color.Transparent, Color(0xE0050608)),
                    ),
                )
                .padding(start = 16.dp, end = 16.dp, top = 16.dp, bottom = 28.dp),
        ) {
            if (thermalForced720) {
                ThermalBanner()
                Spacer(modifier = Modifier.height(12.dp))
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
                Spacer(modifier = Modifier.height(12.dp))
            }

            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceEvenly,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                StatPill(
                    bitrate = bitrateMbps,
                    lensFacing = lensFacing,
                )

                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Box(
                        modifier = Modifier
                            .size(72.dp)
                            .clip(CircleShape)
                            .border(3.dp, Color.White.copy(alpha = 0.85f), CircleShape)
                            .clickable(onClick = onDisconnect)
                            .padding(6.dp),
                        contentAlignment = Alignment.Center,
                    ) {
                        Box(
                            modifier = Modifier
                                .fillMaxSize()
                                .clip(RoundedCornerShape(8.dp))
                                .background(Color(0xE6FF5C6C)),
                        )
                    }
                    Spacer(modifier = Modifier.height(6.dp))
                    Text(
                        text = "断开连接",
                        color = Color(0xFFD7DBE6),
                        fontSize = 11.sp,
                    )
                }

                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    IconButton(
                        onClick = onFlipCamera,
                        modifier = Modifier
                            .size(56.dp)
                            .clip(CircleShape)
                            .background(Color(0x40000000)),
                    ) {
                        Icon(
                            imageVector = Icons.Default.Cameraswitch,
                            contentDescription = "切换前后摄像头",
                            tint = Color.White,
                            modifier = Modifier.size(26.dp),
                        )
                    }
                    Spacer(modifier = Modifier.height(4.dp))
                    Text(
                        text = if (lensFacing == LensFacing.Front) "前置" else "后置",
                        color = Color(0xFFD7DBE6),
                        fontSize = 11.sp,
                    )
                }
            }
        }
    }
}

@Composable
private fun HudBadge(content: @Composable RowScope.() -> Unit) {
    Row(
        modifier = Modifier
            .clip(RoundedCornerShape(999.dp))
            .background(Color(0x8C0A0C10))
            .border(1.dp, Color(0x1FFFFFFF), RoundedCornerShape(999.dp))
            .padding(horizontal = 12.dp, vertical = 8.dp),
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
    content: @Composable () -> Unit,
) {
    IconButton(
        onClick = onClick,
        enabled = enabled,
        modifier = Modifier
            .size(36.dp)
            .clip(CircleShape)
            .background(Color(0x66000000)),
    ) {
        content()
    }
}

@Composable
private fun ResPill(text: String, onClick: () -> Unit) {
    Text(
        text = text,
        modifier = Modifier
            .clip(RoundedCornerShape(999.dp))
            .background(Color(0x8C0A0C10))
            .border(1.dp, Color(0x47FF6A3D), RoundedCornerShape(999.dp))
            .clickable(onClick = onClick)
            .padding(horizontal = 12.dp, vertical = 8.dp),
        color = Color(0xFFFFD7C8),
        fontSize = 12.sp,
        fontWeight = FontWeight.Bold,
        fontFamily = FontFamily.Monospace,
    )
}

@Composable
private fun StatPill(bitrate: String, lensFacing: LensFacing) {
    Column(
        modifier = Modifier
            .clip(RoundedCornerShape(14.dp))
            .background(Color(0x40000000))
            .padding(horizontal = 12.dp, vertical = 10.dp),
        horizontalAlignment = Alignment.Start,
    ) {
        Text(
            text = bitrate,
            color = Color.White,
            fontSize = 14.sp,
            fontWeight = FontWeight.Bold,
        )
        Text(
            text = "30 FPS · ${if (lensFacing == LensFacing.Front) "前置" else "后置"}",
            color = PicooColors.Ready,
            fontSize = 11.sp,
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
