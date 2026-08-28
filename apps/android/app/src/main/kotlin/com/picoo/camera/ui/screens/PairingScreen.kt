package com.picoo.camera.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
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
import com.picoo.camera.ui.theme.PicooFont

/** REQ-PICOO-UI-003 — 配对页，对齐 m-screen-pairing。 */
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
                    colors = listOf(Color(0x24FF6A3D), Color.Transparent),
                    radius = 900f,
                ),
            )
            .padding(horizontal = 18.dp, vertical = 24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Text(
            text = buildString {
                append("连接到 ")
                append(receiverName.ifBlank { "电脑" })
                if (viaQr) append("（扫码）")
            },
            color = PicooColors.Muted,
            fontSize = 14.sp,
        )
        Spacer(modifier = Modifier.height(6.dp))
        Text(
            text = "核对 6 位配对短码",
            color = PicooColors.Text,
            fontFamily = PicooFont.Display,
            fontSize = 22.sp,
            fontWeight = FontWeight.Bold,
            letterSpacing = (-0.3).sp,
        )
        Spacer(modifier = Modifier.height(18.dp))
        Text(
            text = pairingCode.ifBlank { "······" },
            fontFamily = PicooFont.Display,
            fontSize = 51.sp,
            fontWeight = FontWeight.ExtraBold,
            letterSpacing = 10.sp,
            style = androidx.compose.ui.text.TextStyle(
                brush = Brush.verticalGradient(
                    colors = listOf(Color.White, Color(0xFFFFD2BF)),
                ),
            ),
        )
        Spacer(modifier = Modifier.height(4.dp))
        Text(
            text = "短码 60 秒内有效",
            color = PicooColors.Muted,
            fontSize = 13.sp,
        )
        Spacer(modifier = Modifier.height(12.dp))
        Text(
            text = "核对手机与电脑屏幕上是否显示相同数字。",
            color = PicooColors.Muted,
            fontSize = 14.sp,
            lineHeight = 20.sp,
            textAlign = TextAlign.Center,
        )
        Text(
            text = "数字不一致？可能连错设备，请取消。",
            color = Color(0xFFC8873F),
            fontSize = 12.sp,
            modifier = Modifier.padding(top = 4.dp),
            textAlign = TextAlign.Center,
        )
        Spacer(modifier = Modifier.height(24.dp))
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
