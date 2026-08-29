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
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.picoo.camera.ui.components.PicooGhostButton
import com.picoo.camera.ui.components.PicooIconButton
import com.picoo.camera.ui.components.PicooPill
import com.picoo.camera.ui.components.PicooPrimaryButton
import com.picoo.camera.ui.components.Reicon
import com.picoo.camera.ui.components.ReiconIcon
import com.picoo.camera.ui.theme.PicooColors
import com.picoo.camera.ui.theme.PicooFont

/** REQ-PICOO-UI-0001 AC-M-MANUAL-01/02 — mDNS 不可用时手动输入 Receiver 地址。 */
@Composable
fun ManualConnectScreen(
    errorText: String?,
    onBack: () -> Unit,
    onConnect: (host: String, port: Int) -> Unit,
    modifier: Modifier = Modifier,
) {
    var endpoint by remember { mutableStateOf("") }

    Column(
        modifier = modifier
            .fillMaxSize()
            .background(PicooColors.Panel),
    ) {
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .padding(start = 8.dp, end = 16.dp, top = 44.dp, bottom = 10.dp),
        ) {
            PicooIconButton(
                onClick = onBack,
                modifier = Modifier.align(Alignment.CenterStart),
            ) {
                ReiconIcon(
                    icon = Reicon.ChevronLeft,
                    contentDescription = "返回",
                    tint = PicooColors.Text,
                    modifier = Modifier.size(18.dp),
                )
            }
            PicooPill(
                text = "IP 直连",
                modifier = Modifier.align(Alignment.CenterEnd),
            )
        }

        Column(
            modifier = Modifier
                .weight(1f)
                .fillMaxWidth()
                .padding(horizontal = 24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center,
        ) {
            Text(
                text = "输入电脑连接地址",
                color = PicooColors.Text,
                fontFamily = PicooFont.Display,
                fontSize = 22.sp,
                fontWeight = FontWeight.Bold,
            )
            Spacer(modifier = Modifier.height(8.dp))
            Text(
                text = "如果没有自动发现电脑，请输入 Receiver 页面显示的 IP 地址和端口。",
                color = PicooColors.Muted,
                fontSize = 14.sp,
                lineHeight = 20.sp,
                textAlign = TextAlign.Center,
            )
            Spacer(modifier = Modifier.height(24.dp))
            Text(
                text = "电脑连接地址 (IP:端口)",
                color = PicooColors.Muted,
                fontSize = 12.sp,
                fontWeight = FontWeight.SemiBold,
                modifier = Modifier.fillMaxWidth(),
            )
            Spacer(modifier = Modifier.height(8.dp))
            OutlinedTextField(
                value = endpoint,
                onValueChange = { endpoint = it },
                modifier = Modifier.fillMaxWidth(),
                placeholder = { Text("192.168.1.108:4433") },
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
            errorText?.let {
                Spacer(modifier = Modifier.height(10.dp))
                Text(text = it, color = PicooColors.Danger, fontSize = 13.sp)
            }
            Spacer(modifier = Modifier.height(14.dp))
            PicooPrimaryButton(
                text = "连接电脑",
                onClick = {
                    val parts = endpoint.trim().split(":", limit = 2)
                    val host = parts.firstOrNull()?.trim().orEmpty()
                    val port = parts.getOrNull(1)?.trim()?.toIntOrNull() ?: 4433
                    if (host.isNotEmpty()) onConnect(host, port)
                },
            )
            Spacer(modifier = Modifier.height(8.dp))
            PicooGhostButton(text = "取消并返回", onClick = onBack, small = true)
        }
    }
}
