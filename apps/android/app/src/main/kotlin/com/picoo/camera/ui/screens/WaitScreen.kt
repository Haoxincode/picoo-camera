package com.picoo.camera.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextAlign
import com.picoo.camera.ui.components.PicooGhostButton
import com.picoo.camera.ui.components.PicooPrimaryButton
import com.picoo.camera.ui.components.Reicon
import com.picoo.camera.ui.components.ReiconIcon
import com.picoo.camera.ui.theme.PicooTheme

enum class WaitOutcome { Pending, Rejected, Timeout }

/** REQ-PICOO-UI-012 / AC-M-PAIR-03 — explicit pending, rejected and expired states. */
@Composable
fun WaitScreen(
    receiverName: String,
    onCancel: () -> Unit,
    modifier: Modifier = Modifier,
    outcome: WaitOutcome = WaitOutcome.Pending,
    onBackToDevices: () -> Unit = onCancel,
    onRegenerate: () -> Unit = onBackToDevices,
) {
    val colors = PicooTheme.colors
    val dimensions = PicooTheme.dimensions
    Scaffold(
        modifier = modifier.fillMaxSize(),
        containerColor = colors.surfacePage,
    ) { innerPadding ->
        Box(
            modifier = Modifier
                .fillMaxSize()
                .padding(innerPadding),
            contentAlignment = Alignment.Center,
        ) {
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .widthIn(max = dimensions.maxContentWidth)
                    .verticalScroll(rememberScrollState())
                    .padding(dimensions.space24),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(dimensions.space16),
            ) {
                when (outcome) {
                    WaitOutcome.Pending -> {
                        CircularProgressIndicator(
                            modifier = Modifier.size(dimensions.iconHero),
                            color = colors.actionHighlight,
                        )
                        WaitMessage(
                            title = "等待电脑端确认",
                            detail = "请在 $receiverName 的配对提示中确认相同短码。双方确认后会自动开始推流。",
                        )
                        PicooGhostButton(
                            text = "取消连接",
                            onClick = onCancel,
                            modifier = Modifier.fillMaxWidth(),
                        )
                    }
                    WaitOutcome.Rejected -> {
                        ReiconIcon(
                            icon = Reicon.Rejected,
                            contentDescription = null,
                            tint = colors.statusDanger,
                            modifier = Modifier.size(dimensions.iconHero),
                        )
                        WaitMessage(
                            title = "电脑端拒绝了连接",
                            detail = "电脑使用者点击了拒绝。确认电脑归属后可重新发起连接。",
                        )
                        PicooPrimaryButton(
                            text = "返回设备列表",
                            onClick = onBackToDevices,
                            modifier = Modifier.fillMaxWidth(),
                        )
                    }
                    WaitOutcome.Timeout -> {
                        ReiconIcon(
                            icon = Reicon.Expired,
                            contentDescription = null,
                            tint = colors.statusWarning,
                            modifier = Modifier.size(dimensions.iconHero),
                        )
                        WaitMessage(
                            title = "配对已超时",
                            detail = "60 秒内未完成双向确认，短码已失效。请重新发起连接。",
                        )
                        PicooPrimaryButton(
                            text = "重新发起配对",
                            onClick = onRegenerate,
                            modifier = Modifier.fillMaxWidth(),
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun WaitMessage(title: String, detail: String) {
    val colors = PicooTheme.colors
    val dimensions = PicooTheme.dimensions
    Column(
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(dimensions.space8),
    ) {
        Text(text = title, style = MaterialTheme.typography.titleLarge, textAlign = TextAlign.Center)
        Text(
            text = detail,
            color = colors.contentMuted,
            style = MaterialTheme.typography.bodyMedium,
            textAlign = TextAlign.Center,
        )
    }
}
