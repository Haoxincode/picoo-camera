package com.picoo.camera.ui.screens

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
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
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.picoo.camera.ui.QrCodeScanner
import com.picoo.camera.ui.components.PicooGhostButton
import com.picoo.camera.ui.components.PicooPill
import com.picoo.camera.ui.components.PicooPrimaryButton
import com.picoo.camera.ui.theme.PicooColors

/** REQ-PICOO-UI-003 / PUC-003 — 扫码兜底全屏页。 */
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
    modifier: Modifier = Modifier,
) {
    val success = qrSuccessName != null
    Column(
        modifier = modifier
            .fillMaxSize()
            .background(Color(0xFF050607)),
    ) {
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .padding(start = 16.dp, end = 16.dp, top = 48.dp, bottom = 12.dp),
        ) {
            OutlinedButton(
                onClick = onBack,
                shape = RoundedCornerShape(12.dp),
                colors = ButtonDefaults.outlinedButtonColors(contentColor = PicooColors.Text),
                border = androidx.compose.foundation.BorderStroke(
                    1.dp,
                    Color(0x24FFFFFF),
                ),
                modifier = Modifier.align(Alignment.CenterStart),
            ) {
                Text("← 返回", fontWeight = FontWeight.SemiBold, fontSize = 14.sp)
            }
            PicooPill(
                text = "扫码兜底",
                warn = true,
                modifier = Modifier.align(Alignment.CenterEnd),
            )
        }

        Box(
            modifier = Modifier
                .weight(1f)
                .padding(horizontal = 16.dp)
                .clip(RoundedCornerShape(22.dp))
                .background(
                    Brush.verticalGradient(
                        colors = listOf(Color(0xFF1A2030), Color(0xFF0A0C10)),
                    ),
                )
                .border(1.dp, Color(0x14FFFFFF), RoundedCornerShape(22.dp)),
        ) {
            if (cameraGranted && !success) {
                QrCodeScanner(
                    modifier = Modifier.fillMaxSize(),
                    onQrDetected = onQrDetected,
                    onClose = {},
                    showCloseButton = false,
                )
                QrScanOverlay()
            } else if (!cameraGranted) {
                Column(
                    modifier = Modifier
                        .fillMaxSize()
                        .padding(24.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = androidx.compose.foundation.layout.Arrangement.Center,
                ) {
                    Text(
                        text = "扫码需要相机权限",
                        color = PicooColors.Muted,
                        fontSize = 15.sp,
                    )
                    Spacer(modifier = Modifier.height(12.dp))
                    PicooPrimaryButton(text = "授予相机权限", onClick = onRequestCamera)
                }
            } else {
                Box(
                    modifier = Modifier
                        .fillMaxSize()
                        .background(Color(0x66000000)),
                )
            }

            Text(
                text = "对准电脑端 Show QR Code",
                color = Color(0xFFD7DBE6),
                fontSize = 14.sp,
                textAlign = TextAlign.Center,
                modifier = Modifier
                    .align(Alignment.BottomCenter)
                    .padding(bottom = 16.dp, start = 20.dp, end = 20.dp),
            )
        }

        Column(
            modifier = Modifier.padding(start = 20.dp, end = 20.dp, top = 16.dp, bottom = 24.dp),
        ) {
            if (success) {
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
                Spacer(modifier = Modifier.height(10.dp))
            }
            Text(
                text = buildString {
                    append("QR 含 host / port / receiver_id / nonce\n绕过 mDNS，直连 QUIC")
                    if (!qrHostPort.isNullOrBlank()) append("\n$qrHostPort")
                },
                color = PicooColors.Muted,
                fontSize = 13.sp,
                lineHeight = 18.sp,
                textAlign = TextAlign.Center,
                modifier = Modifier.fillMaxWidth(),
            )
            Spacer(modifier = Modifier.height(10.dp))
            if (success) {
                PicooPrimaryButton(text = "继续配对", onClick = onContinuePairing)
            }
            errorText?.let {
                Spacer(modifier = Modifier.height(8.dp))
                Text(text = it, color = PicooColors.Danger, fontSize = 13.sp)
            }
        }
    }
}

@Composable
private fun QrScanOverlay() {
    Box(modifier = Modifier.fillMaxSize()) {
        Box(
            modifier = Modifier
                .fillMaxSize()
                .padding(horizontal = 14.dp, vertical = 18.dp)
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
