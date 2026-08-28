package com.picoo.camera.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.MoreHoriz
import androidx.compose.material.icons.filled.QrCodeScanner
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Wifi
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.picoo.camera.discovery.DiscoveredReceiverRow
import com.picoo.camera.jni.PicooNative
import com.picoo.camera.pairing.TrustedDeviceList
import com.picoo.camera.ui.components.DeviceBadge
import com.picoo.camera.ui.components.DiscoveryPulseDot
import com.picoo.camera.ui.components.PicooGhostButton
import com.picoo.camera.ui.components.PicooIconButton
import com.picoo.camera.ui.components.PicooPill
import com.picoo.camera.ui.components.PicooPrimaryButton
import com.picoo.camera.ui.components.PicooSheet
import com.picoo.camera.ui.components.PicooSheetRow
import com.picoo.camera.ui.theme.PicooColors
import com.picoo.camera.ui.theme.PicooFont

/** REQ-PICOO-UI-003 — 发现页，对齐 HTML 原型 m-screen-devices。 */
@Composable
fun DevicesScreen(
    discoveredList: List<PicooNative.DiscoveredReceiver>,
    pairedDevices: List<PicooNative.TrustedDevice>,
    pairedReceiverIds: Set<String>,
    nearbyWifiGranted: Boolean,
    discoveryComplete: Boolean,
    wifiPillText: String,
    errorText: String?,
    onSelectReceiver: (PicooNative.DiscoveredReceiver) -> Unit,
    onScanQr: () -> Unit,
    onCheckPermissions: () -> Unit,
    onRemovePaired: (PicooNative.TrustedDevice) -> Unit,
    onOfflinePairedClick: (PicooNative.TrustedDevice) -> Unit,
    onRequestNearbyWifi: () -> Unit,
    onOpenSettings: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val empty = discoveredList.isEmpty() && discoveryComplete
    val searching = discoveredList.isEmpty() && !discoveryComplete
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
                .padding(start = 18.dp, end = 12.dp, top = 44.dp, bottom = 10.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = "Picoo Camera",
                color = PicooColors.Text,
                fontFamily = PicooFont.Display,
                fontSize = 21.sp,
                fontWeight = FontWeight.Bold,
                letterSpacing = (-0.3).sp,
            )
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                PicooPill(text = wifiPillText)
                PicooIconButton(onClick = onOpenSettings) {
                    Icon(
                        imageVector = Icons.Default.Settings,
                        contentDescription = "设置",
                        tint = PicooColors.Muted,
                        modifier = Modifier.size(16.dp),
                    )
                }
            }
        }

        Column(modifier = Modifier.padding(horizontal = 18.dp)) {
            Text(
                text = "把手机变成无线摄像头",
                color = PicooColors.Text,
                fontFamily = PicooFont.Display,
                fontSize = 21.sp,
                fontWeight = FontWeight.Bold,
                lineHeight = 26.sp,
                letterSpacing = (-0.3).sp,
            )
            Spacer(modifier = Modifier.height(4.dp))
            Text(
                text = "选择同一 Wi‑Fi 下的电脑，已配对设备点按直连。",
                color = PicooColors.Muted,
                fontSize = 14.sp,
                lineHeight = 20.sp,
            )
            Spacer(modifier = Modifier.height(14.dp))

            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                DiscoveryPulseDot(searching = searching)
                Text(
                    text = if (empty) {
                        "发现完成 · 未找到可用电脑"
                    } else {
                        "正在发现附近电脑…"
                    },
                    color = PicooColors.Muted,
                    fontSize = 13.sp,
                )
            }
            Spacer(modifier = Modifier.height(10.dp))

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
                val pairedMeta = pairedDevices.find { it.deviceId == receiver.receiverId }
                DeviceCard(
                    name = receiver.displayName,
                    meta = if (locallyTrusted && pairedMeta != null) {
                        "在线 · 公钥 ${TrustedDeviceList.shortFingerprint(pairedMeta.certificateFingerprint)} · 点按直连"
                    } else if (locallyTrusted) {
                        "在线 · ${receiver.host} · 点按直连"
                    } else {
                        "${DiscoveredReceiverRow.PLATFORM_WINDOWS} · 首次连接需配对短码"
                    },
                    badge = if (locallyTrusted) "已配对" else "在线",
                    paired = locallyTrusted,
                    offline = false,
                    fingerprint = pairedMeta?.certificateFingerprint,
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
                    meta = "已配对 · 公钥 ${TrustedDeviceList.shortFingerprint(device.certificateFingerprint)} · 上次 ${TrustedDeviceList.formatLastConnected(device.lastConnectedAtMs)}",
                    badge = "不在线",
                    paired = true,
                    offline = true,
                    fingerprint = device.certificateFingerprint,
                    onClick = { onOfflinePairedClick(device) },
                    onRemove = { onRemovePaired(device) },
                )
            }

            if (empty) {
                EmptyDiscoveryHint()
            }

            Spacer(modifier = Modifier.height(14.dp))
            if (empty) {
                PicooPrimaryButton(
                    text = "扫描电脑端二维码连接",
                    onClick = onScanQr,
                )
            } else {
                ScanQrGhostButton(onClick = onScanQr)
            }
            Spacer(modifier = Modifier.height(8.dp))
            PicooGhostButton(
                text = "权限未开？点此检查",
                onClick = onCheckPermissions,
                small = true,
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
private fun ScanQrGhostButton(onClick: () -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(14.dp))
            .border(1.dp, PicooColors.Line, RoundedCornerShape(14.dp))
            .clickable(onClick = onClick)
            .padding(vertical = 13.dp),
        horizontalArrangement = Arrangement.Center,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(
            imageVector = Icons.Default.QrCodeScanner,
            contentDescription = null,
            tint = PicooColors.Text,
            modifier = Modifier.size(17.dp),
        )
        Spacer(modifier = Modifier.width(8.dp))
        Text(
            text = "扫描电脑端二维码连接",
            color = PicooColors.Text,
            fontWeight = FontWeight.Bold,
            fontSize = 15.sp,
        )
    }
}

@Composable
private fun DeviceCard(
    name: String,
    meta: String,
    badge: String,
    paired: Boolean,
    offline: Boolean,
    fingerprint: String? = null,
    onClick: () -> Unit,
    onRemove: (() -> Unit)?,
) {
    var menuOpen by remember { mutableStateOf(false) }
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .height(IntrinsicSize.Min)
            .padding(bottom = 9.dp)
            .clip(RoundedCornerShape(16.dp))
            .background(
                Brush.verticalGradient(
                    colors = listOf(Color(0x08FFFFFF), Color.Transparent),
                ),
            )
            .background(PicooColors.Panel2)
            .border(1.dp, PicooColors.Line, RoundedCornerShape(16.dp))
            .alpha(if (offline) 0.55f else 1f),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(
            modifier = Modifier
                .weight(1f)
                .clickable(enabled = !offline, onClick = onClick)
                .padding(start = 14.dp, top = 13.dp, bottom = 13.dp, end = 4.dp),
        ) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Text(
                    text = name,
                    color = if (offline) PicooColors.Muted else PicooColors.Text,
                    fontFamily = PicooFont.Display,
                    fontSize = 16.sp,
                    fontWeight = FontWeight.Bold,
                )
                DeviceBadge(label = badge, paired = paired && !offline, offline = offline)
            }
            Text(
                text = meta,
                color = PicooColors.Muted,
                fontSize = 12.sp,
                modifier = Modifier.padding(top = 3.dp),
            )
        }
        if (onRemove != null) {
            Box(
                modifier = Modifier
                    .width(42.dp)
                    .fillMaxHeight()
                    .border(
                        width = 1.dp,
                        color = PicooColors.Line,
                        shape = RoundedCornerShape(topEnd = 16.dp, bottomEnd = 16.dp),
                    )
                    .clickable { menuOpen = true },
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    imageVector = Icons.Default.MoreHoriz,
                    contentDescription = "更多操作",
                    tint = PicooColors.Muted,
                )
            }
        }
    }
    if (menuOpen && onRemove != null) {
        PicooSheet(
            title = name,
            description = "已配对的信任电脑 · 公钥指纹: ${
                TrustedDeviceList.shortFingerprint(fingerprint.orEmpty())
            }",
            onDismiss = { menuOpen = false },
        ) {
            PicooSheetRow(
                title = "撤销信任并删除配对",
                subtitle = "下次连接需重新在两端核对 6 位短码",
                danger = true,
                onClick = {
                    menuOpen = false
                    onRemove()
                },
            )
            PicooSheetRow(
                title = "取消",
                onClick = { menuOpen = false },
            )
        }
    }
}

@Composable
private fun EmptyDiscoveryHint() {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(bottom = 12.dp)
            .clip(RoundedCornerShape(16.dp))
            .border(1.dp, Color(0x1FFFFFFF), RoundedCornerShape(16.dp))
            .background(Color(0x05FFFFFF))
            .padding(horizontal = 14.dp, vertical = 20.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Box(
            modifier = Modifier
                .size(44.dp)
                .background(Color(0x1FFF6A3D), RoundedCornerShape(12.dp))
                .border(1.dp, Color(0x40FF6A3D), RoundedCornerShape(12.dp)),
            contentAlignment = Alignment.Center,
        ) {
            Icon(
                imageVector = Icons.Default.Wifi,
                contentDescription = null,
                tint = PicooColors.Accent,
                modifier = Modifier.size(22.dp),
            )
        }
        Spacer(modifier = Modifier.height(10.dp))
        Text(
            text = "未发现附近电脑",
            color = PicooColors.Text,
            fontFamily = PicooFont.Display,
            fontSize = 15.sp,
            fontWeight = FontWeight.Bold,
        )
        Spacer(modifier = Modifier.height(4.dp))
        Text(
            text = "企业网络或路由器可能屏蔽了 mDNS 服务广播",
            color = PicooColors.Muted,
            fontSize = 13.sp,
            lineHeight = 19.sp,
            modifier = Modifier.padding(horizontal = 8.dp),
        )
        Spacer(modifier = Modifier.height(10.dp))
        Column(modifier = Modifier.fillMaxWidth(0.85f)) {
            ChecklistItem("手机与电脑连接同一 Wi‑Fi？")
            ChecklistItem("电脑端 Picoo Camera 已启动？")
            ChecklistItem("改用扫码直连，绕过 mDNS 限制 ↓")
        }
    }
}

@Composable
private fun ChecklistItem(text: String) {
    Row(
        modifier = Modifier.padding(vertical = 2.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Text(text = "·", color = PicooColors.Accent2, fontSize = 13.sp)
        Text(text = text, color = PicooColors.Muted, fontSize = 12.sp, lineHeight = 18.sp)
    }
}
