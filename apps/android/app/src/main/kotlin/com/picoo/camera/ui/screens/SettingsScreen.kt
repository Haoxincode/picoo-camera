package com.picoo.camera.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.picoo.camera.ui.theme.PicooColors

/** REQ-PICOO-UI-003 — 手机端设置：已配对数量、权限提示。 */
@Composable
fun SettingsScreen(
    pairedDeviceCount: Int,
    cameraGranted: Boolean,
    nearbyWifiGranted: Boolean,
    notificationsGranted: Boolean,
    onBack: () -> Unit,
    onCheckPermissions: () -> Unit,
    onOpenPairedDevices: () -> Unit,
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
                .padding(start = 4.dp, end = 16.dp, top = 40.dp, bottom = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            IconButton(onClick = onBack) {
                Icon(
                    imageVector = Icons.AutoMirrored.Filled.ArrowBack,
                    contentDescription = "返回",
                    tint = PicooColors.Text,
                )
            }
            Text(
                text = "手机端设置",
                color = PicooColors.Text,
                fontSize = 20.sp,
                fontWeight = FontWeight.Bold,
            )
        }

        Column(modifier = Modifier.padding(horizontal = 20.dp)) {
            SettingsRow(
                title = "已配对信任电脑",
                description = "管理已固定的对端公钥（PUC-007）",
                value = "$pairedDeviceCount 台 ›",
                onClick = onOpenPairedDevices,
            )
            SettingsRow(
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
            SettingsRow(
                title = "关于 Picoo Camera",
                description = "协议 PCP/1 · Android Sender v0.1.0",
                value = "v0.1.0",
                onClick = {},
            )
            Spacer(modifier = Modifier.height(24.dp))
        }
    }
}

@Composable
private fun SettingsRow(
    title: String,
    description: String,
    value: String,
    onClick: () -> Unit,
    valueColor: androidx.compose.ui.graphics.Color = PicooColors.Muted,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 6.dp)
            .clip(RoundedCornerShape(16.dp))
            .background(PicooColors.Panel2)
            .clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 14.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(modifier = Modifier.weight(1f)) {
            Text(
                text = title,
                color = PicooColors.Text,
                fontSize = 16.sp,
                fontWeight = FontWeight.SemiBold,
            )
            Text(
                text = description,
                color = PicooColors.Muted,
                fontSize = 13.sp,
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
