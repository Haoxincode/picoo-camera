package com.picoo.camera.ui.screens

import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer
import com.picoo.camera.discovery.DiscoveredReceiverRow
import com.picoo.camera.jni.PicooNative
import com.picoo.camera.pairing.TrustedDeviceList
import com.picoo.camera.ui.components.DiscoveryPulseDot
import com.picoo.camera.ui.components.PicooSheet
import com.picoo.camera.ui.components.PicooSheetRow
import com.picoo.camera.ui.components.Reicon
import com.picoo.camera.ui.components.ReiconIcon
import com.picoo.camera.ui.theme.PicooTheme

/**
 * The connection affordance is deliberately a sheet over the camera surface,
 * not a second navigation destination. Offline trusted devices stay in
 * Settings; this sheet only shows receivers that are discoverable now.
 */
@Composable
internal fun ConnectionSheet(
    discoveredList: List<PicooNative.DiscoveredReceiver>,
    pairedDevices: List<PicooNative.TrustedDevice>,
    pairedReceiverIds: Set<String>,
    nearbyWifiGranted: Boolean,
    discoveryComplete: Boolean,
    errorText: String?,
    manualEndpointText: String,
    manualConnecting: Boolean,
    onSelectReceiver: (PicooNative.DiscoveredReceiver) -> Unit,
    onManualConnect: (host: String, port: Int) -> Unit,
    onManualEndpointChange: (String) -> Unit,
    onCheckPermissions: () -> Unit,
    onRequestNearbyWifi: () -> Unit,
    onRestartDiscovery: () -> Unit,
    onStopDiscovery: () -> Unit,
    onDismiss: () -> Unit,
) {
    var manualSheetOpen by rememberSaveable { mutableStateOf(false) }
    val dimensions = PicooTheme.dimensions
    val searching = discoveredList.isEmpty() && !discoveryComplete

    PicooSheet(
        title = "连接电脑",
        description = "同一 Wi‑Fi 下自动发现，也可以输入局域网 IP。",
        onDismiss = {
            manualSheetOpen = false
            onDismiss()
        },
    ) {
        when {
            !nearbyWifiGranted -> {
                PicooSheetRow(
                    title = "允许查找附近电脑",
                    subtitle = "开启附近 Wi‑Fi 设备权限后自动发现电脑",
                    leadingContent = {
                        ReiconIcon(
                            icon = Reicon.Network,
                            contentDescription = null,
                            tint = PicooTheme.colors.actionHighlight,
                            modifier = Modifier.size(PicooTheme.dimensions.iconEmphasis),
                        )
                    },
                    trailingContent = { SheetChevron() },
                    onClick = onRequestNearbyWifi,
                )
            }
            discoveredList.isNotEmpty() -> {
                Text(
                    text = "附近的电脑",
                    color = PicooTheme.colors.contentMuted,
                    style = androidx.compose.material3.MaterialTheme.typography.labelLarge,
                    modifier = Modifier.padding(bottom = dimensions.space8),
                )
                discoveredList.forEach { receiver ->
                val trusted = pairedReceiverIds.contains(receiver.receiverId)
                val paired = pairedDevices.firstOrNull { it.deviceId == receiver.receiverId }
                PicooSheetRow(
                    title = receiver.displayName,
                    subtitle = if (trusted && paired != null) {
                        "已配对 · 公钥 ${TrustedDeviceList.shortFingerprint(paired.certificateFingerprint)}"
                    } else {
                        "${DiscoveredReceiverRow.platformLabel(receiver.platform)} · 首次连接需核对短码"
                    },
                        selected = trusted,
                        leadingContent = {
                            ReiconIcon(
                                icon = Reicon.ReceiverDevice,
                                contentDescription = null,
                                tint = if (trusted) {
                                    PicooTheme.colors.actionHighlight
                                } else {
                                    PicooTheme.colors.contentMuted
                                },
                                modifier = Modifier.size(dimensions.iconEmphasis),
                            )
                        },
                        trailingContent = { SheetChevron() },
                        onClick = {
                            onSelectReceiver(receiver)
                            onDismiss()
                        },
                    )
                }
            }
            searching -> {
                PicooSheetRow(
                    title = "正在搜索附近电脑",
                    subtitle = "确认电脑端已打开，并连接到同一 Wi‑Fi",
                    leadingContent = { DiscoveryPulseDot(searching = true) },
                    trailingContent = {
                        ReiconIcon(
                            icon = Reicon.Rejected,
                            contentDescription = "停止搜索",
                            tint = PicooTheme.colors.contentMuted,
                            modifier = Modifier.size(dimensions.iconStandard),
                        )
                    },
                    onClick = onStopDiscovery,
                )
            }
            else -> {
                PicooSheetRow(
                    title = "未发现附近电脑",
                    subtitle = "确认电脑端已打开，并连接到同一 Wi‑Fi",
                    leadingContent = {
                        ReiconIcon(
                            icon = Reicon.ReceiverDevice,
                            contentDescription = null,
                            tint = PicooTheme.colors.contentMuted,
                            modifier = Modifier.size(dimensions.iconEmphasis),
                        )
                    },
                    trailingContent = {
                        ReiconIcon(
                            icon = Reicon.RefreshDiscovery,
                            contentDescription = "重新搜索",
                            tint = PicooTheme.colors.actionHighlight,
                            modifier = Modifier.size(dimensions.iconStandard),
                        )
                    },
                    onClick = onRestartDiscovery,
                )
                PicooSheetRow(
                    title = "输入局域网 IP",
                    subtitle = "直接连接电脑端显示的地址",
                    leadingContent = {
                        ReiconIcon(
                            icon = Reicon.SecureConnection,
                            contentDescription = null,
                            tint = PicooTheme.colors.actionHighlight,
                            modifier = Modifier.size(dimensions.iconEmphasis),
                        )
                    },
                    trailingContent = { SheetChevron() },
                    onClick = { manualSheetOpen = true },
                )
            }
        }

        if (errorText != null) {
            Text(
                text = errorText,
                color = PicooTheme.colors.statusDanger,
                style = androidx.compose.material3.MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(top = dimensions.space12),
            )
        }

        if (nearbyWifiGranted && discoveredList.isNotEmpty()) {
            PicooSheetRow(
                title = "输入局域网 IP",
                subtitle = "直接连接电脑端显示的地址",
                leadingContent = {
                    ReiconIcon(
                        icon = Reicon.SecureConnection,
                        contentDescription = null,
                        tint = PicooTheme.colors.actionHighlight,
                        modifier = Modifier.size(dimensions.iconEmphasis),
                    )
                },
                trailingContent = { SheetChevron() },
                onClick = { manualSheetOpen = true },
            )
        }

        if (nearbyWifiGranted && discoveryComplete && discoveredList.isEmpty()) {
            PicooSheetRow(
                title = "检查网络与发现权限",
                subtitle = "如果电脑已打开但仍未出现，请检查系统权限",
                leadingContent = {
                    ReiconIcon(
                        icon = Reicon.Network,
                        contentDescription = null,
                        tint = PicooTheme.colors.contentMuted,
                        modifier = Modifier.size(dimensions.iconEmphasis),
                    )
                },
                trailingContent = { SheetChevron() },
                onClick = onCheckPermissions,
            )
        }
    }

    if (manualSheetOpen) {
        ManualConnectSheet(
            endpoint = manualEndpointText,
            connecting = manualConnecting,
            errorText = errorText,
            onDismiss = { manualSheetOpen = false },
            onConnect = { host, port ->
                onManualConnect(host, port)
                manualSheetOpen = false
            },
            onEndpointChange = onManualEndpointChange,
        )
    }
}

@Composable
private fun SheetChevron() {
    ReiconIcon(
        icon = Reicon.NavigateBack,
        contentDescription = null,
        tint = PicooTheme.colors.contentMuted,
        modifier = Modifier
            .size(PicooTheme.dimensions.iconCompact)
            .graphicsLayer(rotationZ = 180f),
    )
}
