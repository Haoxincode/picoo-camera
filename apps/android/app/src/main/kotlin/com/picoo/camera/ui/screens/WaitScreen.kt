package com.picoo.camera.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
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
import com.picoo.camera.ui.components.PicooPrimaryButton
import com.picoo.camera.ui.components.Reicon
import com.picoo.camera.ui.components.ReiconIcon
import com.picoo.camera.ui.theme.PicooColors
import com.picoo.camera.ui.theme.PicooFont

enum class WaitOutcome {
    Pending,
    Rejected,
    Timeout,
}

/** REQ-PICOO-UI-003 — 等待电脑确认，对齐 m-screen-wait。 */
@Composable
fun WaitScreen(
    receiverName: String,
    outcome: WaitOutcome = WaitOutcome.Pending,
    onCancel: () -> Unit,
    onBackToDevices: () -> Unit = onCancel,
    onRegenerate: () -> Unit = onBackToDevices,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier = modifier
            .fillMaxSize()
            .background(PicooColors.Panel)
            .background(
                Brush.radialGradient(
                    colors = listOf(PicooColors.Accent2.copy(alpha = 0.14f), Color.Transparent),
                    radius = 900f,
                ),
            )
            .padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        when (outcome) {
            WaitOutcome.Pending -> {
                CircularProgressIndicator(
                    modifier = Modifier.size(40.dp),
                    color = PicooColors.Accent,
                    strokeWidth = 3.dp,
                )
                Spacer(modifier = Modifier.height(16.dp))
                Text(
                    text = "等待电脑端确认…",
                    color = PicooColors.Text,
                    fontFamily = PicooFont.Display,
                    fontSize = 22.sp,
                    fontWeight = FontWeight.Bold,
                )
                Spacer(modifier = Modifier.height(12.dp))
                Text(
                    text = "请在电脑端弹窗中点击「确认配对」\n双方确认后将固定公钥并自动开始推流",
                    color = PicooColors.Muted,
                    fontSize = 14.sp,
                    lineHeight = 20.sp,
                    textAlign = TextAlign.Center,
                )
                if (receiverName.isNotBlank()) {
                    Spacer(modifier = Modifier.height(8.dp))
                    Text(
                        text = "连接到 $receiverName",
                        color = PicooColors.MutedDark,
                        fontSize = 13.sp,
                    )
                }
                Spacer(modifier = Modifier.height(24.dp))
                PicooGhostButton(text = "取消", onClick = onCancel)
            }
            WaitOutcome.Rejected -> {
                WaitIconBox(warn = false, danger = true) {
                    ReiconIcon(
                        icon = Reicon.Xmark,
                        contentDescription = null,
                        tint = PicooColors.Danger,
                    )
                }
                Spacer(modifier = Modifier.height(14.dp))
                Text(
                    text = "电脑端拒绝了连接",
                    fontFamily = PicooFont.Display,
                    fontSize = 22.sp,
                    fontWeight = FontWeight.Bold,
                    color = PicooColors.Text,
                )
                Spacer(modifier = Modifier.height(8.dp))
                Text(
                    text = "电脑使用者点击了拒绝。\n确认电脑归属后可重新发起连接。",
                    color = PicooColors.Muted,
                    fontSize = 14.sp,
                    lineHeight = 20.sp,
                    textAlign = TextAlign.Center,
                )
                Spacer(modifier = Modifier.height(20.dp))
                PicooPrimaryButton(text = "返回设备列表", onClick = onBackToDevices)
            }
            WaitOutcome.Timeout -> {
                WaitIconBox(warn = true, danger = false) {
                    ReiconIcon(
                        icon = Reicon.Clock,
                        contentDescription = null,
                        tint = PicooColors.Warn,
                    )
                }
                Spacer(modifier = Modifier.height(14.dp))
                Text(
                    text = "配对已超时",
                    fontFamily = PicooFont.Display,
                    fontSize = 22.sp,
                    fontWeight = FontWeight.Bold,
                    color = PicooColors.Text,
                )
                Spacer(modifier = Modifier.height(8.dp))
                Text(
                    text = "60 秒内未完成双向确认，短码已失效。\n请重新发起连接。",
                    color = PicooColors.Muted,
                    fontSize = 14.sp,
                    lineHeight = 20.sp,
                    textAlign = TextAlign.Center,
                )
                Spacer(modifier = Modifier.height(20.dp))
                PicooPrimaryButton(text = "重新发起配对", onClick = onRegenerate)
            }
        }
    }
}

@Composable
private fun WaitIconBox(
    warn: Boolean,
    danger: Boolean,
    content: @Composable () -> Unit,
) {
    val bg = when {
        danger -> PicooColors.DangerBg
        warn -> Color(0x24F0C14A)
        else -> PicooColors.Panel2
    }
    val border = when {
        danger -> Color(0x4DFF5C6C)
        warn -> Color(0x4DF0C14A)
        else -> PicooColors.Line
    }
    Box(
        modifier = Modifier
            .size(52.dp)
            .background(bg, RoundedCornerShape(16.dp))
            .border(1.dp, border, RoundedCornerShape(16.dp)),
        contentAlignment = Alignment.Center,
    ) {
        content()
    }
}
