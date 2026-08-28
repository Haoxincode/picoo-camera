package com.picoo.camera.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.picoo.camera.ui.components.PicooGhostButton
import com.picoo.camera.ui.components.PicooPrimaryButton
import com.picoo.camera.ui.theme.PicooColors

/** REQ-PICOO-UI-003 — 配对页：大号六位短码 + 确认/取消。 */
@Composable
fun PairingScreen(
    receiverName: String,
    pairingCode: String,
    viaQr: Boolean,
    errorText: String?,
    onConfirm: () -> Unit,
    onCancel: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier = modifier
            .fillMaxSize()
            .background(PicooColors.Panel)
            .background(
                Brush.radialGradient(
                    colors = listOf(Color(0x29FF6A3D), Color.Transparent),
                    radius = 800f,
                ),
            )
            .padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        val kicker = buildString {
            append("连接到 ")
            append(receiverName.ifBlank { "电脑" })
            if (viaQr) append("（扫码）")
        }
        Text(
            text = kicker,
            color = PicooColors.Muted,
            fontSize = 15.sp,
        )
        Spacer(modifier = Modifier.height(8.dp))
        Text(
            text = "核对 6 位配对短码",
            color = PicooColors.Text,
            fontSize = 24.sp,
            fontWeight = FontWeight.Bold,
            letterSpacing = (-0.3).sp,
        )
        Spacer(modifier = Modifier.height(28.dp))
        Text(
            text = pairingCode.ifBlank { "······" },
            fontSize = 52.sp,
            fontWeight = FontWeight.ExtraBold,
            letterSpacing = 12.sp,
            style = androidx.compose.ui.text.TextStyle(
                brush = Brush.verticalGradient(
                    colors = listOf(
                        PicooColors.Text,
                        Color(0xFFFFD2BF),
                    ),
                ),
            ),
        )
        Spacer(modifier = Modifier.height(10.dp))
        Text(
            text = "短码 60 秒内有效",
            color = PicooColors.Accent2,
            fontSize = 13.sp,
            fontWeight = FontWeight.SemiBold,
        )
        Spacer(modifier = Modifier.height(16.dp))
        Text(
            text = "核对手机与电脑屏幕上是否显示相同数字。\n数字不一致？可能连错设备，请取消。",
            color = PicooColors.Muted,
            fontSize = 15.sp,
            lineHeight = 22.sp,
            textAlign = TextAlign.Center,
        )
        Spacer(modifier = Modifier.height(28.dp))
        PicooPrimaryButton(
            text = "两端数字一致，确认配对",
            onClick = onConfirm,
            enabled = pairingCode.isNotEmpty(),
        )
        Spacer(modifier = Modifier.height(10.dp))
        PicooGhostButton(text = "取消", onClick = onCancel)
        errorText?.let {
            Spacer(modifier = Modifier.height(16.dp))
            Text(
                text = it,
                color = PicooColors.Danger,
                fontSize = 13.sp,
                modifier = Modifier.fillMaxWidth(),
                textAlign = TextAlign.Center,
            )
        }
    }
}
