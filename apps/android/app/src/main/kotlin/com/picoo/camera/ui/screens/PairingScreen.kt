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
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import com.picoo.camera.ui.components.PicooGhostButton
import com.picoo.camera.ui.components.PicooPrimaryButton
import com.picoo.camera.ui.components.Reicon
import com.picoo.camera.ui.components.ReiconIcon
import com.picoo.camera.ui.formatPairingCode
import com.picoo.camera.ui.theme.PicooFont
import com.picoo.camera.ui.theme.PicooTheme

/** REQ-PICOO-UI-012 / AC-M-PAIR-01..02 — secure pairing in Control context. */
@Composable
fun PairingScreen(
    receiverName: String,
    pairingCode: String,
    remainingSeconds: Int,
    expired: Boolean,
    errorText: String?,
    onConfirm: () -> Unit,
    onRegenerate: () -> Unit,
    onCancel: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = PicooTheme.colors
    val dimensions = PicooTheme.dimensions
    val formattedCode = formatPairingCode(pairingCode)
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
                ReiconIcon(
                    icon = Reicon.SecureConnection,
                    contentDescription = null,
                    tint = colors.actionHighlight,
                    modifier = Modifier.size(dimensions.iconHero),
                )
                Column(
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(dimensions.space4),
                ) {
                    Text(
                        text = receiverName.ifBlank { "电脑" },
                        color = colors.contentMuted,
                        style = MaterialTheme.typography.bodyMedium,
                    )
                    Text(
                        text = "核对 6 位配对短码",
                        style = MaterialTheme.typography.titleLarge,
                    )
                }
                Text(
                    text = formattedCode.ifBlank { "··· ···" },
                    color = if (expired) colors.contentMuted else colors.actionHighlight,
                    fontFamily = PicooFont.Mono,
                    style = MaterialTheme.typography.headlineLarge,
                    fontWeight = FontWeight.ExtraBold,
                    textAlign = TextAlign.Center,
                )
                Text(
                    text = if (expired) "短码已过期" else "剩余 ${remainingSeconds.coerceAtLeast(0)} 秒",
                    color = if (expired) colors.statusDanger else colors.contentMuted,
                    fontFamily = PicooFont.Mono,
                    style = MaterialTheme.typography.bodyMedium,
                )
                Text(
                    text = if (expired) {
                        "60 秒内未完成双向确认，请重新发起配对。"
                    } else {
                        "确认手机与电脑屏幕上显示相同数字；不一致时请立即取消。"
                    },
                    color = colors.contentMuted,
                    style = MaterialTheme.typography.bodyMedium,
                    textAlign = TextAlign.Center,
                )
                if (expired) {
                    PicooPrimaryButton(
                        text = "重新发起配对",
                        onClick = onRegenerate,
                        modifier = Modifier.fillMaxWidth(),
                    )
                } else {
                    PicooPrimaryButton(
                        text = "两端数字一致，确认配对",
                        onClick = onConfirm,
                        enabled = pairingCode.isNotEmpty(),
                        modifier = Modifier.fillMaxWidth(),
                    )
                }
                PicooGhostButton(
                    text = "取消",
                    onClick = onCancel,
                    modifier = Modifier.fillMaxWidth(),
                )
                errorText?.let { message ->
                    Text(
                        text = message,
                        color = colors.statusDanger,
                        style = MaterialTheme.typography.bodyMedium,
                        textAlign = TextAlign.Center,
                    )
                }
            }
        }
    }
}
