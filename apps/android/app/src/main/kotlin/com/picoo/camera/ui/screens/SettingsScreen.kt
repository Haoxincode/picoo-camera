package com.picoo.camera.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.picoo.camera.ui.components.PicooIconButton
import com.picoo.camera.ui.theme.PicooColors
import com.picoo.camera.ui.theme.PicooFont

/** REQ-PICOO-UI-003 — 手机端设置，对齐 m-screen-settings。 */
@Composable
fun SettingsScreen(
    pairedDeviceCount: Int,
    cameraGranted: Boolean,
    nearbyWifiGranted: Boolean,
    notificationsGranted: Boolean,
    autoConnectEnabled: Boolean,
    defaultResolutionLabel: String,
    onBack: () -> Unit,
    onCheckPermissions: () -> Unit,
    onOpenPairedDevices: () -> Unit,
    onToggleAutoConnect: () -> Unit,
    onOpenDefaultResolution: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val permissionsReady = cameraGranted && nearbyWifiGranted && notificationsGranted
    Column(
        modifier = modifier
            .fillMaxSize()
            .background(PicooColors.Panel)
            .verticalScroll(rememberScrollState()),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(start = 8.dp, end = 18.dp, top = 44.dp, bottom = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            PicooIconButton(onClick = onBack) {
                Icon(
                    imageVector = Icons.AutoMirrored.Filled.ArrowBack,
                    contentDescription = "返回",
                    tint = PicooColors.Text,
                    modifier = Modifier.size(18.dp),
                )
            }
            Text(
                text = "手机端设置",
                color = PicooColors.Text,
                fontFamily = PicooFont.Display,
                fontSize = 21.sp,
                fontWeight = FontWeight.Bold,
                modifier = Modifier.weight(1f),
            )
            Box(modifier = Modifier.size(32.dp))
        }

        Column(modifier = Modifier.padding(horizontal = 18.dp)) {
            SettingsToggleRow(
                title = "打开 App 自动直连",
                description = "上次连接的电脑在线时，3秒倒计时后自动直连",
                checked = autoConnectEnabled,
                onClick = onToggleAutoConnect,
            )
            SettingsValueRow(
                title = "默认初始画质",
                description = "新连接建立时的起步分辨率档位",
                value = "$defaultResolutionLabel ›",
                onClick = onOpenDefaultResolution,
            )
            SettingsValueRow(
                title = "已配对信任电脑",
                description = "管理已固定的对端公钥（PUC-007）",
                value = "$pairedDeviceCount 台 ›",
                onClick = onOpenPairedDevices,
            )
            SettingsValueRow(
                title = "权限就绪状态",
                description = "相机 / 局域网广播 / 前台保活",
                value = if (permissionsReady) "全部就绪 ›" else "待检查 ›",
                valueColor = if (permissionsReady) PicooColors.Ready else PicooColors.Warn,
                onClick = onCheckPermissions,
            )
            if (!permissionsReady) {
                Spacer(modifier = Modifier.height(8.dp))
                PermissionHint(
                    cameraGranted = cameraGranted,
                    nearbyWifiGranted = nearbyWifiGranted,
                    notificationsGranted = notificationsGranted,
                )
            }
            SettingsValueRow(
                title = "关于 Picoo Camera",
                description = "协议 PCP/1 · Rust Core v1.0.0",
                value = "v0.1.0",
                onClick = {},
            )
            Spacer(modifier = Modifier.height(24.dp))
        }
    }
}

@Composable
private fun SettingsToggleRow(
    title: String,
    description: String,
    checked: Boolean,
    onClick: () -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 4.dp)
            .clip(RoundedCornerShape(14.dp))
            .background(PicooColors.Panel2)
            .border(1.dp, PicooColors.Line, RoundedCornerShape(14.dp))
            .clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 14.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(modifier = Modifier.weight(1f).padding(end = 12.dp)) {
            Text(
                text = title,
                color = PicooColors.Text,
                fontSize = 15.sp,
                fontWeight = FontWeight.SemiBold,
            )
            Text(
                text = description,
                color = PicooColors.Muted,
                fontSize = 12.sp,
                modifier = Modifier.padding(top = 4.dp),
            )
        }
        PicooSwitch(checked = checked)
    }
}

@Composable
private fun SettingsValueRow(
    title: String,
    description: String,
    value: String,
    onClick: () -> Unit,
    valueColor: androidx.compose.ui.graphics.Color = PicooColors.Muted,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 4.dp)
            .clip(RoundedCornerShape(14.dp))
            .background(PicooColors.Panel2)
            .border(1.dp, PicooColors.Line, RoundedCornerShape(14.dp))
            .clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 14.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(modifier = Modifier.weight(1f).padding(end = 12.dp)) {
            Text(
                text = title,
                color = PicooColors.Text,
                fontSize = 15.sp,
                fontWeight = FontWeight.SemiBold,
            )
            Text(
                text = description,
                color = PicooColors.Muted,
                fontSize = 12.sp,
                modifier = Modifier.padding(top = 4.dp),
            )
        }
        Text(
            text = value,
            color = valueColor,
            fontSize = 14.sp,
            fontWeight = FontWeight.Medium,
        )
    }
}

@Composable
private fun PicooSwitch(checked: Boolean) {
    Box(
        modifier = Modifier
            .size(width = 44.dp, height = 26.dp)
            .clip(RoundedCornerShape(999.dp))
            .background(if (checked) PicooColors.Accent else PicooColors.Panel3)
            .border(1.dp, PicooColors.Line, RoundedCornerShape(999.dp))
            .padding(3.dp),
        contentAlignment = if (checked) Alignment.CenterEnd else Alignment.CenterStart,
    ) {
        Box(
            modifier = Modifier
                .size(20.dp)
                .clip(CircleShape)
                .background(PicooColors.Text),
        )
    }
}

@Composable
private fun PermissionHint(
    cameraGranted: Boolean,
    nearbyWifiGranted: Boolean,
    notificationsGranted: Boolean,
) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(14.dp))
            .background(PicooColors.Panel2)
            .padding(14.dp),
    ) {
        Text(
            text = "权限检查清单",
            color = PicooColors.Muted,
            fontSize = 12.sp,
            fontWeight = FontWeight.Bold,
            letterSpacing = 0.5.sp,
        )
        Spacer(modifier = Modifier.height(8.dp))
        PermissionLine("相机", cameraGranted)
        PermissionLine("附近 Wi‑Fi 设备", nearbyWifiGranted)
        PermissionLine("通知（前台保活）", notificationsGranted)
    }
}

@Composable
private fun PermissionLine(label: String, granted: Boolean) {
    Text(
        text = if (granted) "✓ $label" else "○ $label — 未授予",
        color = if (granted) PicooColors.Ready else PicooColors.Warn,
        fontSize = 13.sp,
        modifier = Modifier.padding(vertical = 2.dp),
    )
}
