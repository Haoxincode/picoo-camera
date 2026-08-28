package com.picoo.camera.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.picoo.camera.media.LensFacing
import com.picoo.camera.ui.CameraPreviewSurface
import com.picoo.camera.ui.components.PicooDangerButton
import com.picoo.camera.ui.components.PicooPrimaryButton
import com.picoo.camera.ui.theme.PicooColors

/** REQ-PICOO-UI-003 / REQ-PICOO-UI-005 — 传输页：全屏预览 + 控件 + 过热横幅。 */
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

        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(start = 16.dp, end = 16.dp, top = 44.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            StreamChip(text = receiverName.ifBlank { "电脑" })
            if (linkQualityChip.isNotEmpty()) {
                StreamChip(text = linkQualityChip)
            }
        }

        Column(
            modifier = Modifier
                .align(Alignment.BottomCenter)
                .fillMaxWidth()
                .background(
                    Brush.verticalGradient(
                        colors = listOf(Color.Transparent, Color(0xE0050608)),
                    ),
                )
                .padding(start = 16.dp, end = 16.dp, top = 16.dp, bottom = 22.dp),
        ) {
            if (thermalForced720) {
                ThermalBanner()
                Spacer(modifier = Modifier.height(10.dp))
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
                Spacer(modifier = Modifier.height(10.dp))
            }

            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                MetricChip("${resolutionLabel}30")
                MetricChip(bitrateMbps)
                MetricChip(if (lensFacing == LensFacing.Front) "前置" else "后置")
                MetricChip(
                    if (localPreviewMirrored) "预览镜像开" else "预览镜像关",
                )
            }
            Spacer(modifier = Modifier.height(12.dp))

            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                ControlButton(
                    text = "前后摄",
                    onClick = onFlipCamera,
                    modifier = Modifier.weight(1f),
                )
                ControlButton(
                    text = resolutionLabel,
                    onClick = onToggleResolution,
                    modifier = Modifier.weight(1f),
                    active = resolutionLabel == "1080p",
                )
                ControlButton(
                    text = "镜像",
                    onClick = onToggleMirror,
                    modifier = Modifier.weight(1f),
                    active = localPreviewMirrored,
                )
                ControlButton(
                    text = "EV ${if (exposureEv > 0) "+" else ""}$exposureEv",
                    onClick = onEvStep,
                    modifier = Modifier.weight(1f),
                    enabled = evSupported,
                )
            }
            Spacer(modifier = Modifier.height(10.dp))
            PicooDangerButton(text = "断开连接", onClick = onDisconnect)
        }
    }
}

@Composable
private fun StreamChip(text: String) {
    Text(
        text = text,
        modifier = Modifier
            .clip(RoundedCornerShape(999.dp))
            .background(Color(0x8C0A0C10))
            .border(1.dp, Color(0x1FFFFFFF), RoundedCornerShape(999.dp))
            .padding(horizontal = 12.dp, vertical = 8.dp),
        color = Color(0xFFD7DBE6),
        fontSize = 13.sp,
        fontWeight = FontWeight.SemiBold,
    )
}

@Composable
private fun MetricChip(text: String) {
    Text(
        text = text,
        modifier = Modifier
            .clip(RoundedCornerShape(10.dp))
            .background(Color(0x0FFFFFFF))
            .border(1.dp, Color(0x14FFFFFF), RoundedCornerShape(10.dp))
            .padding(horizontal = 8.dp, vertical = 6.dp),
        color = Color(0xFFD7DBE6),
        fontSize = 12.sp,
    )
}

@Composable
private fun ControlButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    active: Boolean = false,
    enabled: Boolean = true,
) {
    val border = if (active) Color(0x8CFF6A3D) else Color(0x1AFFFFFF)
    val bg = if (active) Color(0x2EFF6A3D) else Color(0x0FFFFFFF)
    val fg = if (active) Color(0xFFFFD7C8) else PicooColors.Text
    Box(
        modifier = modifier
            .clip(RoundedCornerShape(14.dp))
            .background(bg)
            .border(1.dp, border, RoundedCornerShape(14.dp))
            .clickable(enabled = enabled, onClick = onClick)
            .padding(vertical = 12.dp, horizontal = 6.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = text,
            color = if (enabled) fg else PicooColors.Muted,
            fontSize = 12.sp,
            fontWeight = FontWeight.SemiBold,
        )
    }
}

@Composable
private fun ThermalBanner() {
    Text(
        text = "设备偏热，已强制 720p 以稳住推流",
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
