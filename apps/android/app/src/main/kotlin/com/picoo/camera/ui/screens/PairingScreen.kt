package com.picoo.camera.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.border
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
            .padding(20.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(
                    Brush.radialGradient(
                        colors = listOf(Color(0x29FF6A3D), Color.Transparent),
                        radius = 800f,
                    ),
                ),
        ) {
            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(20.dp),
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
                    text = "确认相同短码",
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
                    text = "请与电脑屏幕上的六位数字核对。\n一致后再确认，未配对不会传视频。",
                    color = PicooColors.Muted,
                    fontSize = 15.sp,
                    lineHeight = 22.sp,
                    textAlign = TextAlign.Center,
                )
                Spacer(modifier = Modifier.height(28.dp))
                PicooPrimaryButton(
                    text = "确认配对并开始",
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
    }
}
