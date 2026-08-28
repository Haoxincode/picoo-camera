package com.picoo.camera.ui.screens

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
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.picoo.camera.ui.QrCodeScanner
import com.picoo.camera.ui.components.PicooGhostButton
import com.picoo.camera.ui.components.PicooPill
import com.picoo.camera.ui.components.PicooPrimaryButton
import com.picoo.camera.ui.theme.PicooColors
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween

/** REQ-PICOO-UI-003 / PUC-003 — 全屏扫码 + 相机被拒时手动 IP 兜底。 */
@Composable
fun QrScanScreen(
    cameraGranted: Boolean,
    qrSuccessName: String?,
    qrHostPort: String?,
    errorText: String?,
    onBack: () -> Unit,
    onQrDetected: (String) -> Unit,
    onContinuePairing: () -> Unit,
    onRequestCamera: () -> Unit,
    onManualConnect: (host: String, port: Int) -> Unit,
    modifier: Modifier = Modifier,
) {
    val success = qrSuccessName != null
    var manualIp by remember { mutableStateOf("192.168.1.108:4433") }

    Box(
        modifier = modifier
            .fillMaxSize()
            .background(Color(0xFF050607)),
    ) {
        if (cameraGranted && !success) {
            QrCodeScanner(
                modifier = Modifier.fillMaxSize(),
                onQrDetected = onQrDetected,
                onClose = {},
                showCloseButton = false,
            )
            QrScanOverlay()
        }

        Column(modifier = Modifier.fillMaxSize()) {
            RowTopBar(onBack = onBack)

            if (!cameraGranted) {
                ManualIpFallback(
                    manualIp = manualIp,
                    onManualIpChange = { manualIp = it },
                    onConnect = {
                        val parts = manualIp.trim().split(":")
                        val host = parts.firstOrNull()?.trim().orEmpty()
                        val port = parts.getOrNull(1)?.trim()?.toIntOrNull() ?: 4433
                        if (host.isNotEmpty()) onManualConnect(host, port)
                    },
                    onBack = onBack,
                    onRequestCamera = onRequestCamera,
                    modifier = Modifier.weight(1f),
                )
            } else if (success) {
                Column(
                    modifier = Modifier
                        .weight(1f)
                        .padding(24.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = androidx.compose.foundation.layout.Arrangement.Center,
                ) {
                    Text(
                        text = "识别成功 · $qrSuccessName",
                        color = PicooColors.Ready,
                        fontWeight = FontWeight.SemiBold,
                        fontSize = 15.sp,
                        modifier = Modifier
                            .fillMaxWidth()
                            .clip(RoundedCornerShape(16.dp))
                            .background(Color(0x1F3ECF8E))
                            .border(1.dp, Color(0x473ECF8E), RoundedCornerShape(16.dp))
                            .padding(14.dp),
                        textAlign = TextAlign.Center,
                    )
                    if (!qrHostPort.isNullOrBlank()) {
                        Spacer(modifier = Modifier.height(8.dp))
                        Text(
                            text = qrHostPort,
                            color = PicooColors.Muted,
                            fontSize = 13.sp,
                        )
                    }
                    Spacer(modifier = Modifier.height(16.dp))
                    PicooPrimaryButton(text = "继续配对", onClick = onContinuePairing)
                }
            } else {
                Box(modifier = Modifier.weight(1f)) {
                    Text(
                        text = "对准电脑端 Show QR Code\n识别成功将自动建立 QUIC 连接",
                        color = Color(0xFFD7DBE6),
                        fontSize = 14.sp,
                        lineHeight = 20.sp,
                        textAlign = TextAlign.Center,
                        modifier = Modifier
                            .align(Alignment.BottomCenter)
                            .padding(bottom = 32.dp, start = 24.dp, end = 24.dp),
                    )
                }
            }

            errorText?.let {
                Text(
                    text = it,
                    color = PicooColors.Danger,
                    fontSize = 13.sp,
                    modifier = Modifier.padding(horizontal = 20.dp, vertical = 8.dp),
                )
            }
        }
    }
}

@Composable
private fun RowTopBar(onBack: () -> Unit) {
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .padding(start = 8.dp, end = 16.dp, top = 44.dp, bottom = 12.dp),
    ) {
        IconButton(
            onClick = onBack,
            modifier = Modifier.align(Alignment.CenterStart),
        ) {
            Icon(
                imageVector = Icons.AutoMirrored.Filled.ArrowBack,
                contentDescription = "返回",
                tint = PicooColors.Text,
            )
        }
        PicooPill(
            text = "扫码直连",
            warn = true,
            modifier = Modifier.align(Alignment.CenterEnd),
        )
    }
}

@Composable
private fun ManualIpFallback(
    manualIp: String,
    onManualIpChange: (String) -> Unit,
    onConnect: () -> Unit,
    onBack: () -> Unit,
    onRequestCamera: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier = modifier
            .fillMaxWidth()
            .padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = androidx.compose.foundation.layout.Arrangement.Center,
    ) {
        Text(
            text = "未开启相机权限",
            color = PicooColors.Text,
            fontSize = 20.sp,
            fontWeight = FontWeight.Bold,
        )
        Spacer(modifier = Modifier.height(8.dp))
        Text(
            text = "无法使用取景扫码。可在下方直接输入电脑端显示的连接地址。",
            color = PicooColors.Muted,
            fontSize = 14.sp,
            lineHeight = 20.sp,
            textAlign = TextAlign.Center,
        )
        Spacer(modifier = Modifier.height(20.dp))
        Text(
            text = "电脑连接地址 (IP:端口)",
            color = PicooColors.Muted,
            fontSize = 13.sp,
            fontWeight = FontWeight.SemiBold,
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(modifier = Modifier.height(8.dp))
        OutlinedTextField(
            value = manualIp,
            onValueChange = onManualIpChange,
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
            colors = OutlinedTextFieldDefaults.colors(
                focusedTextColor = PicooColors.Text,
                unfocusedTextColor = PicooColors.Text,
                focusedBorderColor = PicooColors.Accent,
                unfocusedBorderColor = PicooColors.Line,
            ),
        )
        Spacer(modifier = Modifier.height(12.dp))
        PicooPrimaryButton(text = "直连电脑", onClick = onConnect)
        Spacer(modifier = Modifier.height(8.dp))
        PicooGhostButton(text = "授予相机权限", onClick = onRequestCamera)
        Spacer(modifier = Modifier.height(8.dp))
        PicooGhostButton(text = "取消并返回", onClick = onBack)
    }
}

@Composable
private fun QrScanOverlay() {
    Box(modifier = Modifier.fillMaxSize()) {
        Box(
            modifier = Modifier
                .fillMaxSize()
                .padding(horizontal = 24.dp, vertical = 80.dp)
                .border(2.dp, Color(0xBFFF6A3D), RoundedCornerShape(16.dp)),
        )
        val transition = rememberInfiniteTransition(label = "scan")
        val offsetY by transition.animateFloat(
            initialValue = 0.22f,
            targetValue = 0.72f,
            animationSpec = infiniteRepeatable(
                animation = tween(2200, easing = LinearEasing),
                repeatMode = RepeatMode.Reverse,
            ),
            label = "scanline",
        )
        Box(
            modifier = Modifier
                .fillMaxWidth(0.72f)
                .height(2.dp)
                .align(Alignment.TopCenter)
                .offset(y = (offsetY * 400).dp)
                .background(
                    Brush.horizontalGradient(
                        colors = listOf(Color.Transparent, PicooColors.Accent, Color.Transparent),
                    ),
                ),
        )
    }
}
