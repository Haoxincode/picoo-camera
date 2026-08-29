package com.picoo.camera.ui.screens

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowLeft
import androidx.compose.material3.Icon
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
import com.picoo.camera.ui.components.PicooIconButton
import com.picoo.camera.ui.components.PicooPill
import com.picoo.camera.ui.components.PicooPrimaryButton
import com.picoo.camera.ui.theme.PicooColors
import com.picoo.camera.ui.theme.PicooFont

/** REQ-PICOO-UI-003 / PUC-003 — 全屏扫码，对齐 m-screen-qr。 */
@Composable
fun QrScanScreen(
    cameraGranted: Boolean,
    qrSuccessName: String?,
    qrHostPort: String?,
    errorText: String?,
    onBack: () -> Unit,
    onQrDetected: (String) -> Unit,
    onRequestCamera: () -> Unit,
    onManualConnect: (host: String, port: Int) -> Unit,
    modifier: Modifier = Modifier,
) {
    val success = qrSuccessName != null
    var manualMode by remember { mutableStateOf(!cameraGranted) }
    var manualIp by remember { mutableStateOf("192.168.1.108:4433") }

    Box(
        modifier = modifier
            .fillMaxSize()
            .background(PicooColors.LiveBg),
    ) {
        Column(modifier = Modifier.fillMaxSize()) {
            RowTopBar(onBack = onBack)

            when {
                manualMode || !cameraGranted -> {
                    ManualIpPanel(
                        manualIp = manualIp,
                        onManualIpChange = { manualIp = it },
                        onConnect = {
                            val parts = manualIp.trim().split(":")
                            val host = parts.firstOrNull()?.trim().orEmpty()
                            val port = parts.getOrNull(1)?.trim()?.toIntOrNull() ?: 4433
                            if (host.isNotEmpty()) onManualConnect(host, port)
                        },
                        onBack = onBack,
                        onRequestCamera = {
                            onRequestCamera()
                            manualMode = false
                        },
                        modifier = Modifier.weight(1f),
                    )
                }
                success -> {
                    ConnectingPanel(
                        qrSuccessName = qrSuccessName.orEmpty(),
                        qrHostPort = qrHostPort,
                        modifier = Modifier.weight(1f),
                    )
                }
                else -> {
                    Box(modifier = Modifier.weight(1f)) {
                        QrCodeScanner(
                            modifier = Modifier.fillMaxSize(),
                            onQrDetected = onQrDetected,
                            onClose = {},
                            showCloseButton = false,
                        )
                        // 扫描线会持续重组；QrCodeScanner 只在 factory 绑相机，不得跟着重绑。
                        QrViewportChrome()
                    }
                }
            }

            if (cameraGranted && !success && !manualMode) {
                Text(
                    text = "无法扫码？手动输入连接地址",
                    color = PicooColors.Muted,
                    fontSize = 13.sp,
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(bottom = 20.dp)
                        .clickable { manualMode = true },
                    textAlign = TextAlign.Center,
                )
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
            .padding(start = 8.dp, end = 16.dp, top = 44.dp, bottom = 10.dp),
    ) {
        PicooIconButton(
            onClick = onBack,
            modifier = Modifier.align(Alignment.CenterStart),
        ) {
            Icon(
                    imageVector = Icons.AutoMirrored.Filled.KeyboardArrowLeft,
                contentDescription = "返回",
                tint = PicooColors.Text,
                modifier = Modifier.size(18.dp),
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
private fun QrViewportChrome() {
    Box(
        modifier = Modifier
            .fillMaxSize()
            .padding(horizontal = 14.dp, vertical = 8.dp),
    ) {
        Box(
            modifier = Modifier
                .fillMaxSize()
                .clip(RoundedCornerShape(20.dp))
                .border(2.dp, Color(0xBFFF6A3D), RoundedCornerShape(20.dp)),
        )
        val transition = rememberInfiniteTransition(label = "scan")
        val offsetY by transition.animateFloat(
            initialValue = 0.15f,
            targetValue = 0.75f,
            animationSpec = infiniteRepeatable(
                animation = tween(2200, easing = LinearEasing),
                repeatMode = RepeatMode.Reverse,
            ),
            label = "scanline",
        )
        Box(
            modifier = Modifier
                .fillMaxWidth(0.65f)
                .height(2.dp)
                .align(Alignment.TopCenter)
                .offset(y = (offsetY * 420).dp)
                .background(
                    Brush.horizontalGradient(
                        colors = listOf(Color.Transparent, PicooColors.Accent, Color.Transparent),
                    ),
                ),
        )
        Column(
            modifier = Modifier
                .align(Alignment.BottomCenter)
                .padding(bottom = 28.dp, start = 24.dp, end = 24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text(
                text = "对准电脑端 Show QR Code",
                color = PicooColors.Text,
                fontSize = 14.sp,
                fontWeight = FontWeight.SemiBold,
                textAlign = TextAlign.Center,
            )
            Text(
                text = "识别成功将自动建立 QUIC 连接",
                color = PicooColors.Muted,
                fontSize = 12.sp,
                textAlign = TextAlign.Center,
                modifier = Modifier.padding(top = 4.dp),
            )
        }
    }
}

@Composable
private fun ConnectingPanel(
    qrSuccessName: String,
    qrHostPort: String?,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier = modifier
            .fillMaxWidth()
            .padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
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
                fontFamily = PicooFont.Mono,
            )
        }
        Spacer(modifier = Modifier.height(16.dp))
        Text(
            text = "正在建立 QUIC 连接…",
            color = PicooColors.Muted,
            fontSize = 14.sp,
            textAlign = TextAlign.Center,
        )
    }
}

@Composable
private fun ManualIpPanel(
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
            fontFamily = PicooFont.Display,
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
            fontSize = 12.sp,
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
            shape = RoundedCornerShape(12.dp),
            colors = OutlinedTextFieldDefaults.colors(
                focusedTextColor = PicooColors.Text,
                unfocusedTextColor = PicooColors.Text,
                focusedBorderColor = PicooColors.Accent,
                unfocusedBorderColor = PicooColors.Line,
                focusedContainerColor = PicooColors.Panel2,
                unfocusedContainerColor = PicooColors.Panel2,
            ),
        )
        Spacer(modifier = Modifier.height(12.dp))
        PicooPrimaryButton(text = "直连电脑", onClick = onConnect)
        Spacer(modifier = Modifier.height(8.dp))
        PicooGhostButton(text = "授予相机权限", onClick = onRequestCamera)
        Spacer(modifier = Modifier.height(8.dp))
        PicooGhostButton(text = "取消并返回", onClick = onBack, small = true)
    }
}
