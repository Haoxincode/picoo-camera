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
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
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
import com.picoo.camera.ui.components.ReadinessBadge
import com.picoo.camera.ui.theme.PicooColors

/** REQ-PICOO-UI-003 — 发现页：Ready/Paired 列表、空态 QR、权限提示。 */
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
    modifier: Modifier = Modifier,
) {
    val empty = discoveredList.isEmpty() && discoveryComplete
    Column(
        modifier = modifier
            .fillMaxSize()
            .background(PicooColors.Panel)
            .verticalScroll(rememberScrollState()),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(start = 20.dp, end = 20.dp, top = 48.dp, bottom = 12.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = "Picoo",
                color = PicooColors.Text,
                fontSize = 22.sp,
                fontWeight = FontWeight.Bold,
                letterSpacing = (-0.5).sp,
            )
            PicooPill(text = "局域网")
        }

        Column(modifier = Modifier.padding(horizontal = 20.dp)) {
            Text(
                text = "把手机变成\n电脑摄像头",
                color = PicooColors.Text,
                fontSize = 28.sp,
                fontWeight = FontWeight.Bold,
                lineHeight = 32.sp,
                letterSpacing = (-0.5).sp,
            )
            Spacer(modifier = Modifier.height(8.dp))
            Text(
                text = "选择同一 Wi‑Fi 下的电脑。已配对设备可一键直连。",
                color = PicooColors.Muted,
                fontSize = 15.sp,
                lineHeight = 22.sp,
            )
            Spacer(modifier = Modifier.height(18.dp))

            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                Box(
                    modifier = Modifier
                        .size(8.dp)
                        .background(PicooColors.Accent, RoundedCornerShape(50)),
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

            if (!empty) {
                discoveredList.forEach { receiver ->
                    val locallyTrusted = pairedReceiverIds.contains(receiver.receiverId)
                    val readiness = DiscoveredReceiverRow.readinessLabel(
                        receiver.pairingState,
                        locallyTrusted,
                    )
                    DeviceCard(
                        name = receiver.displayName,
                        meta = if (locallyTrusted) {
                            "${DiscoveredReceiverRow.PLATFORM_WINDOWS} · 上次连接 ${TrustedDeviceList.formatLastConnected(
                                pairedDevices.find { it.deviceId == receiver.receiverId }
                                    ?.lastConnectedAtMs ?: 0L,
                            )}"
                        } else {
                            "${DiscoveredReceiverRow.PLATFORM_WINDOWS} · 首次连接需配对"
                        },
                        badge = readiness,
                        paired = locallyTrusted,
                        onClick = { onSelectReceiver(receiver) },
                    )
                }
            }

            if (empty) {
                EmptyDiscoveryHint()
            }

            if (pairedDevices.isNotEmpty()) {
                Text(
                    text = "已配对",
                    color = PicooColors.Muted,
                    fontSize = 12.sp,
                    fontWeight = FontWeight.Bold,
                    letterSpacing = 1.sp,
                    modifier = Modifier.padding(top = 18.dp, bottom = 10.dp),
                )
                pairedDevices.forEach { device ->
                    DeviceCard(
                        name = device.deviceName,
                        meta = "点按可删除并要求重新配对",
                        badge = "信任",
                        paired = true,
                        onClick = { onRemovePaired(device) },
                    )
                }
            }

            Spacer(modifier = Modifier.height(16.dp))
            PicooGhostButton(
                text = "扫描二维码连接",
                onClick = onScanQr,
                primary = empty,
            )
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
    onClick: () -> Unit,
) {
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
            .background(PicooColors.Panel2)
            .border(1.dp, PicooColors.Line, RoundedCornerShape(18.dp))
            .clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 14.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(modifier = Modifier.weight(1f)) {
            Text(
                text = name,
                color = PicooColors.Text,
                fontSize = 17.sp,
                fontWeight = FontWeight.Bold,
            )
            Text(
                text = meta,
                color = PicooColors.Muted,
                fontSize = 13.sp,
                modifier = Modifier.padding(top = 4.dp),
            )
        }
        ReadinessBadge(label = badge, paired = paired)
    }
}

@Composable
private fun EmptyDiscoveryHint() {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(bottom = 14.dp)
            .clip(RoundedCornerShape(18.dp))
            .border(
                1.dp,
                Color(0x1FFFFFFF),
                RoundedCornerShape(18.dp),
            )
            .background(Color(0x05FFFFFF))
            .padding(horizontal = 12.dp, vertical = 28.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Box(
            modifier = Modifier
                .size(48.dp)
                .background(Color(0x1FFF6A3D), RoundedCornerShape(14.dp))
                .border(1.dp, Color(0x40FF6A3D), RoundedCornerShape(14.dp)),
            contentAlignment = Alignment.Center,
        ) {
            Text(text = "📡", fontSize = 22.sp)
        }
        Spacer(modifier = Modifier.height(12.dp))
        Text(
            text = "未发现附近电脑\n可能是企业 Wi‑Fi 屏蔽了 mDNS",
            color = PicooColors.Muted,
            fontSize = 14.sp,
            lineHeight = 20.sp,
        )
    }
}
