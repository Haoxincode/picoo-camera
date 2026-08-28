package com.picoo.camera.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.CircularProgressIndicator
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
import com.picoo.camera.ui.theme.PicooColors

/** REQ-PICOO-UI-003 — 手机确认配对后等待电脑端确认。 */
@Composable
fun WaitScreen(
    receiverName: String,
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
        CircularProgressIndicator(
            modifier = Modifier.size(48.dp),
            color = PicooColors.Accent,
            strokeWidth = 3.dp,
        )
        Spacer(modifier = Modifier.height(28.dp))
        Text(
            text = "等待电脑端确认…",
            color = PicooColors.Text,
            fontSize = 24.sp,
            fontWeight = FontWeight.Bold,
            letterSpacing = (-0.3).sp,
        )
        Spacer(modifier = Modifier.height(12.dp))
        Text(
            text = buildString {
                append("请在电脑端 GPUI 弹窗中点击「确认配对」\n")
                append("双方确认后将固定公钥并自动开始推流")
                if (receiverName.isNotBlank()) {
                    append("\n\n连接到 ")
                    append(receiverName)
                }
            },
            color = PicooColors.Muted,
            fontSize = 15.sp,
            lineHeight = 22.sp,
            textAlign = TextAlign.Center,
        )
        Spacer(modifier = Modifier.height(32.dp))
        PicooGhostButton(text = "取消", onClick = onCancel)
    }
}
