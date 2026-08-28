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
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Wifi
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.picoo.camera.discovery.DiscoveredReceiverRow
import com.picoo.camera.jni.PicooNative
import com.picoo.camera.pairing.TrustedDeviceList
import com.picoo.camera.ui.components.PicooGhostButton
import com.picoo.camera.ui.components.PicooPill
import com.picoo.camera.ui.components.PicooPrimaryButton
import com.picoo.camera.ui.components.ReadinessBadge
import com.picoo.camera.ui.theme.PicooColors

/** REQ-PICOO-UI-003 — 发现页：Material 顶栏、设备卡片、空态清单、扫码 CTA。 */
@Composable
fun DevicesScreen(
    discoveredList: List<PicooNative.DiscoveredReceiver>,
    pairedDevices: List<PicooNative.TrustedDevice>,
    pairedReceiverIds: Set<String>,
    nearbyWifiGranted: Boolean,
    discoveryComplete: Boolean,
    errorText: String?,
    onSelectReceiver: (PicooNative.DiscoveredReceiver) -> Unit,
    onScanQr: () -> Unit,
    onCheckPermissions: () -> Unit,
    onRemovePaired: (PicooNative.TrustedDevice) -> Unit,
    onRequestNearbyWifi: () -> Unit,
    onOpenSettings: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val empty = discoveredList.isEmpty() && discoveryComplete
    val discoveredIds = discoveredList.map { it.receiverId }.toSet()
    val offlinePaired = pairedDevices.filter { it.deviceId !in discoveredIds }

    Column(
        modifier = modifier
            .fillMaxSize()
            .background(PicooColors.Panel)
            .verticalScroll(rememberScrollState()),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(start = 20.dp, end = 12.dp, top = 44.dp, bottom = 8.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = "Picoo Camera",
                color = PicooColors.Text,
                fontSize = 20.sp,
                fontWeight = FontWeight.Bold,
                letterSpacing = (-0.3).sp,
            )
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                PicooPill(
                    text = "Wi‑Fi · 局域网",
                    modifier = Modifier.padding(end = 4.dp),
                )
                IconButton(onClick = onOpenSettings) {
                    Icon(
                        imageVector = Icons.Default.Settings,
                        contentDescription = "设置",
                        tint = PicooColors.Muted,
                    )
                }
            }
        }

        Column(modifier = Modifier.padding(horizontal = 20.dp)) {
            Text(
                text = "把手机变成无线摄像头",
                color = PicooColors.Text,
                fontSize = 26.sp,
                fontWeight = FontWeight.Bold,
                lineHeight = 32.sp,
                letterSpacing = (-0.5).sp,
            )
            Spacer(modifier = Modifier.height(8.dp))
            Text(
                text = "选择同一 Wi‑Fi 下的电脑，已配对设备点按直连。",
                color = PicooColors.Muted,
                fontSize = 15.sp,
                lineHeight = 22.sp,
            )
            Spacer(modifier = Modifier.height(16.dp))

            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                Box(
                    modifier = Modifier
                        .size(8.dp)
                        .background(
                            if (empty) PicooColors.Muted else PicooColors.Accent,
                            RoundedCornerShape(50),
                        ),
                )
                Text(
                    text = if (empty) {
                        "发现完成 · 未找到可用电脑"
                    } else {
                        "正在发现附近电脑…"
                    },
                    color = PicooColors.Muted,
                    fontSize = 14.sp,
                )
            }
            Spacer(modifier = Modifier.height(14.dp))

            if (!nearbyWifiGranted) {
                Text(
                    text = "Android 13+ 需要「附近 Wi‑Fi 设备」权限才能发现电脑",
                    color = PicooColors.Warn,
                    fontSize = 13.sp,
                    modifier = Modifier.padding(bottom = 8.dp),
                )
                PicooGhostButton(text = "授予附近 Wi‑Fi 权限", onClick = onRequestNearbyWifi)
                Spacer(modifier = Modifier.height(12.dp))
            }

            discoveredList.forEach { receiver ->
                val locallyTrusted = pairedReceiverIds.contains(receiver.receiverId)
                val badge = when {
                    locallyTrusted -> "已配对"
                    else -> "在线"
                }
                val meta = if (locallyTrusted) {
                    "在线 · ${receiver.host} · 点按直连"
                } else {
                    "${DiscoveredReceiverRow.PLATFORM_WINDOWS} · 首次连接需配对短码"
                }
                DeviceCard(
                    name = receiver.displayName,
                    meta = meta,
                    badge = badge,
                    paired = locallyTrusted,
                    offline = false,
                    onClick = { onSelectReceiver(receiver) },
                    onRemove = if (locallyTrusted) {
                        {
                            pairedDevices.find { it.deviceId == receiver.receiverId }
                                ?.let(onRemovePaired)
                        }
                    } else {
                        null
                    },
                )
            }

            offlinePaired.forEach { device ->
                DeviceCard(
                    name = device.deviceName,
                    meta = "已配对 · 上次连接 ${TrustedDeviceList.formatLastConnected(device.lastConnectedAtMs)}",
                    badge = "不在线",
                    paired = true,
                    offline = true,
                    onClick = {},
                    onRemove = { onRemovePaired(device) },
                )
            }

            if (empty) {
                EmptyDiscoveryHint()
            }

            Spacer(modifier = Modifier.height(16.dp))
            if (empty) {
                PicooPrimaryButton(
                    text = "扫描电脑端二维码连接",
                    onClick = onScanQr,
                )
            } else {
                PicooGhostButton(
                    text = "扫描电脑端二维码连接",
                    onClick = onScanQr,
                )
            }
            Spacer(modifier = Modifier.height(10.dp))
            PicooGhostButton(
                text = "权限未开？点此检查",
                onClick = onCheckPermissions,
            )
            errorText?.let {
                Spacer(modifier = Modifier.height(12.dp))
                Text(text = it, color = PicooColors.Danger, fontSize = 13.sp)
            }
            Spacer(modifier = Modifier.height(24.dp))
        }
    }
}

@Composable
private fun DeviceCard(
    name: String,
    meta: String,
    badge: String,
    paired: Boolean,
    offline: Boolean,
    onClick: () -> Unit,
    onRemove: (() -> Unit)?,
) {
    var menuOpen by remember { mutableStateOf(false) }
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(bottom = 10.dp)
            .clip(RoundedCornerShape(18.dp))
            .background(
                Brush.verticalGradient(
                    colors = listOf(Color(0x08FFFFFF), Color.Transparent),
                ),
            )
            .background(if (offline) Color(0xFF181C24) else PicooColors.Panel2)
            .border(1.dp, PicooColors.Line, RoundedCornerShape(18.dp))
            .clickable(enabled = !offline, onClick = onClick)
            .padding(start = 16.dp, end = 4.dp, top = 14.dp, bottom = 14.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(
            modifier = Modifier
                .weight(1f)
                .padding(end = 8.dp),
        ) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Text(
                    text = name,
                    color = if (offline) PicooColors.Muted else PicooColors.Text,
                    fontSize = 17.sp,
                    fontWeight = FontWeight.Bold,
                )
                ReadinessBadge(
                    label = badge,
                    paired = paired && !offline,
                    offline = offline,
                )
            }
            Text(
                text = meta,
                color = PicooColors.Muted,
                fontSize = 13.sp,
                modifier = Modifier.padding(top = 4.dp),
            )
        }
        if (onRemove != null) {
            Box {
                IconButton(onClick = { menuOpen = true }) {
                    Icon(
                        imageVector = Icons.Default.MoreVert,
                        contentDescription = "更多操作",
                        tint = PicooColors.Muted,
                    )
                }
                DropdownMenu(
                    expanded = menuOpen,
                    onDismissRequest = { menuOpen = false },
                ) {
                    DropdownMenuItem(
                        text = {
                            Text(
                                text = "撤销信任并删除配对",
                                color = PicooColors.DangerText,
                            )
                        },
                        onClick = {
                            menuOpen = false
                            onRemove()
                        },
                    )
                }
            }
        }
    }
}

@Composable
private fun EmptyDiscoveryHint() {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(bottom = 14.dp)
            .clip(RoundedCornerShape(18.dp))
            .border(1.dp, Color(0x1FFFFFFF), RoundedCornerShape(18.dp))
            .background(Color(0x05FFFFFF))
            .padding(horizontal = 16.dp, vertical = 24.dp),
    ) {
        Box(
            modifier = Modifier
                .size(48.dp)
                .background(Color(0x1FFF6A3D), RoundedCornerShape(14.dp))
                .border(1.dp, Color(0x40FF6A3D), RoundedCornerShape(14.dp)),
            contentAlignment = Alignment.Center,
        ) {
            Icon(
                imageVector = Icons.Default.Wifi,
                contentDescription = null,
                tint = PicooColors.Accent,
                modifier = Modifier.size(24.dp),
            )
        }
        Spacer(modifier = Modifier.height(12.dp))
        Text(
            text = "未发现附近电脑",
            color = PicooColors.Text,
            fontSize = 16.sp,
            fontWeight = FontWeight.Bold,
        )
        Spacer(modifier = Modifier.height(6.dp))
        Text(
            text = "企业网络或路由器可能屏蔽了 mDNS 服务广播",
            color = PicooColors.Muted,
            fontSize = 14.sp,
            lineHeight = 20.sp,
        )
        Spacer(modifier = Modifier.height(12.dp))
        ChecklistItem("手机与电脑连接同一 Wi‑Fi？")
        ChecklistItem("电脑端 Picoo Camera 已启动？")
        ChecklistItem("改用扫码直连，绕过 mDNS 限制 ↓")
    }
}

@Composable
private fun ChecklistItem(text: String) {
    Text(
        text = "· $text",
        color = PicooColors.Muted,
        fontSize = 13.sp,
        lineHeight = 20.sp,
        modifier = Modifier.padding(vertical = 2.dp),
    )
}
