package com.picoo.camera.ui.screens

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.picoo.camera.discovery.DiscoveredReceiverRow
import com.picoo.camera.jni.PicooNative
import com.picoo.camera.pairing.TrustedDeviceList
import com.picoo.camera.ui.components.DeviceBadge
import com.picoo.camera.ui.components.PicooGhostButton
import com.picoo.camera.ui.components.PicooIconButton
import com.picoo.camera.ui.components.PicooPill
import com.picoo.camera.ui.components.PicooPrimaryButton
import com.picoo.camera.ui.components.PicooSheet
import com.picoo.camera.ui.components.PicooSheetRow
import com.picoo.camera.ui.components.Reicon
import com.picoo.camera.ui.components.ReiconIcon
import com.picoo.camera.ui.theme.PicooTheme

private data class DeviceRowModel(
    val name: String,
    val meta: String,
    val badge: String,
    val paired: Boolean,
    val offline: Boolean,
    val fingerprint: String?,
    val onClick: () -> Unit,
    val onRemove: (() -> Unit)?,
)

/** REQ-PICOO-UI-012 / AC-M-DISC-01..05 — native, token-driven discovery. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DevicesScreen(
    discoveredList: List<PicooNative.DiscoveredReceiver>,
    pairedDevices: List<PicooNative.TrustedDevice>,
    pairedReceiverIds: Set<String>,
    nearbyWifiGranted: Boolean,
    discoveryComplete: Boolean,
    wifiPillText: String,
    errorText: String?,
    manualEndpointText: String,
    manualConnecting: Boolean,
    onSelectReceiver: (PicooNative.DiscoveredReceiver) -> Unit,
    onManualConnect: (host: String, port: Int) -> Unit,
    onManualEndpointChange: (String) -> Unit,
    onCheckPermissions: () -> Unit,
    onRemovePaired: (PicooNative.TrustedDevice) -> Unit,
    onOfflinePairedClick: (PicooNative.TrustedDevice) -> Unit,
    onRequestNearbyWifi: () -> Unit,
    onOpenSettings: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = PicooTheme.colors
    val dimensions = PicooTheme.dimensions
    val empty = discoveredList.isEmpty() && discoveryComplete
    val searching = discoveredList.isEmpty() && !discoveryComplete
    val discoveredIds = discoveredList.map { it.receiverId }.toSet()
    var manualSheetOpen by rememberSaveable { mutableStateOf(false) }
    var managedDevice by remember { mutableStateOf<DeviceRowModel?>(null) }
    var pendingRemoval by remember { mutableStateOf<DeviceRowModel?>(null) }

    val rows = buildList {
        discoveredList.forEach { receiver ->
            val locallyTrusted = pairedReceiverIds.contains(receiver.receiverId)
            val pairedMeta = pairedDevices.find { it.deviceId == receiver.receiverId }
            add(
                DeviceRowModel(
                    name = receiver.displayName,
                    meta = if (locallyTrusted && pairedMeta != null) {
                        "在线 · 公钥 ${TrustedDeviceList.shortFingerprint(pairedMeta.certificateFingerprint)}"
                    } else if (locallyTrusted) {
                        "在线 · ${receiver.host}"
                    } else {
                        "${DiscoveredReceiverRow.PLATFORM_WINDOWS} · 首次连接需核对短码"
                    },
                    badge = if (locallyTrusted) "已配对" else "在线",
                    paired = locallyTrusted,
                    offline = false,
                    fingerprint = pairedMeta?.certificateFingerprint,
                    onClick = { onSelectReceiver(receiver) },
                    onRemove = pairedMeta?.let { device -> { onRemovePaired(device) } },
                ),
            )
        }
        pairedDevices.filter { it.deviceId !in discoveredIds }.forEach { device ->
            add(
                DeviceRowModel(
                    name = device.deviceName,
                    meta = "上次 ${TrustedDeviceList.formatLastConnected(device.lastConnectedAtMs)} · 公钥 ${TrustedDeviceList.shortFingerprint(device.certificateFingerprint)}",
                    badge = "不在线",
                    paired = true,
                    offline = true,
                    fingerprint = device.certificateFingerprint,
                    onClick = { onOfflinePairedClick(device) },
                    onRemove = { onRemovePaired(device) },
                ),
            )
        }
    }

    Scaffold(
        modifier = modifier.fillMaxSize(),
        containerColor = colors.surfacePage,
        topBar = {
            TopAppBar(
                title = {
                    Text(
                        text = "Picoo Camera",
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        style = MaterialTheme.typography.titleMedium,
                    )
                },
                actions = {
                    PicooPill(text = wifiPillText, neutral = true)
                    PicooIconButton(onClick = onOpenSettings) {
                        ReiconIcon(
                            icon = Reicon.Settings,
                            contentDescription = "设置",
                            modifier = Modifier.size(dimensions.iconStandard),
                        )
                    }
                },
            )
        },
    ) { innerPadding ->
        Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.TopCenter) {
            LazyColumn(
                modifier = Modifier
                    .fillMaxWidth()
                    .widthIn(max = dimensions.maxContentWidth),
                contentPadding = PaddingValues(
                    start = dimensions.space16,
                    end = dimensions.space16,
                    top = innerPadding.calculateTopPadding() + dimensions.space16,
                    bottom = innerPadding.calculateBottomPadding() + dimensions.space24,
                ),
                verticalArrangement = Arrangement.spacedBy(dimensions.space16),
            ) {
                item {
                    Column(verticalArrangement = Arrangement.spacedBy(dimensions.space4)) {
                        Text(
                            text = "把手机变成无线摄像头",
                            style = MaterialTheme.typography.headlineLarge,
                        )
                        Text(
                            text = "选择同一 Wi‑Fi 下的电脑，已配对设备点按直连。",
                            color = colors.contentMuted,
                            style = MaterialTheme.typography.bodyMedium,
                        )
                    }
                }

                item {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(dimensions.space8),
                    ) {
                        ReiconIcon(
                            icon = Reicon.Discovering,
                            contentDescription = null,
                            tint = if (searching) colors.actionHighlight else colors.statusSuccess,
                            modifier = Modifier.size(dimensions.iconStandard),
                        )
                        Text(
                            text = when {
                                empty -> "暂未找到可连接的电脑"
                                searching -> "正在搜索同一 Wi‑Fi 下的电脑…"
                                else -> "已找到可连接的电脑"
                            },
                            color = colors.contentMuted,
                            style = MaterialTheme.typography.bodyMedium,
                        )
                    }
                }

                if (!nearbyWifiGranted) {
                    item {
                        Surface(
                            color = colors.statusWarning.copy(alpha = 0.10f),
                            shape = RoundedCornerShape(dimensions.radiusSurface),
                        ) {
                            Column(
                                modifier = Modifier.padding(dimensions.space16),
                                verticalArrangement = Arrangement.spacedBy(dimensions.space12),
                            ) {
                                Text(
                                    text = "需要附近 Wi‑Fi 设备权限才能发现电脑",
                                    color = colors.statusWarning,
                                    style = MaterialTheme.typography.bodyMedium,
                                )
                                PicooGhostButton(
                                    text = "授予权限",
                                    onClick = onRequestNearbyWifi,
                                    modifier = Modifier.fillMaxWidth(),
                                )
                            }
                        }
                    }
                }

                if (rows.isNotEmpty()) {
                    item {
                        Text(
                            text = "可连接的电脑",
                            color = colors.contentMuted,
                            style = MaterialTheme.typography.labelLarge,
                        )
                    }
                    item {
                        Surface(
                            color = colors.surfaceGroup,
                            shape = RoundedCornerShape(dimensions.radiusSurface),
                        ) {
                            Column {
                                rows.forEachIndexed { index, row ->
                                    DeviceRow(
                                        row = row,
                                        onManage = { managedDevice = row },
                                    )
                                    if (index != rows.lastIndex) {
                                        HorizontalDivider(
                                            modifier = Modifier.padding(
                                                start = dimensions.touchTarget + dimensions.space12,
                                            ),
                                            color = colors.borderDefault,
                                        )
                                    }
                                }
                            }
                        }
                    }
                } else if (empty) {
                    item { EmptyDiscoveryHint() }
                }

                item {
                    Column(verticalArrangement = Arrangement.spacedBy(dimensions.space8)) {
                        PicooGhostButton(
                            text = "输入 IP 地址直连",
                            onClick = { manualSheetOpen = true },
                            modifier = Modifier.fillMaxWidth(),
                        )
                        PicooGhostButton(
                            text = "检查发现权限",
                            onClick = onCheckPermissions,
                            modifier = Modifier.fillMaxWidth(),
                            small = true,
                        )
                    }
                }

                errorText?.let { message ->
                    item {
                        Text(
                            text = message,
                            color = colors.statusDanger,
                            style = MaterialTheme.typography.bodyMedium,
                        )
                    }
                }
            }
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
            },
            onEndpointChange = onManualEndpointChange,
        )
    }

    managedDevice?.let { row ->
        PicooSheet(
            title = row.name,
            description = "已配对信任电脑 · 公钥 ${TrustedDeviceList.shortFingerprint(row.fingerprint.orEmpty())}",
            onDismiss = { managedDevice = null },
        ) {
            if (row.onRemove != null) {
                PicooSheetRow(
                    title = "撤销信任并删除配对",
                    subtitle = "下次连接需重新在两端核对 6 位短码",
                    danger = true,
                    onClick = {
                        managedDevice = null
                        pendingRemoval = row
                    },
                )
            }
            PicooSheetRow(title = "取消", onClick = { managedDevice = null })
        }
    }

    pendingRemoval?.let { row ->
        TrustedDeviceRemovalDialog(
            deviceName = row.name,
            onDismiss = { pendingRemoval = null },
            onConfirm = {
                pendingRemoval = null
                row.onRemove?.invoke()
            },
        )
    }
}

@Composable
private fun DeviceRow(
    row: DeviceRowModel,
    onManage: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = PicooTheme.colors
    val dimensions = PicooTheme.dimensions
    Row(
        modifier = modifier
            .fillMaxWidth()
            .defaultMinSize(minHeight = dimensions.touchTarget)
            .alpha(if (row.offline) 0.72f else 1f),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Row(
            modifier = Modifier
                .weight(1f)
                .clickable(onClick = row.onClick)
                .semantics { role = Role.Button }
                .padding(horizontal = dimensions.space12, vertical = dimensions.space12),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(dimensions.space12),
        ) {
            ReiconIcon(
                icon = Reicon.ReceiverDevice,
                contentDescription = null,
                tint = if (row.offline) colors.contentMuted else colors.actionHighlight,
                modifier = Modifier.size(dimensions.iconEmphasis),
            )
            Column(modifier = Modifier.weight(1f)) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(dimensions.space8),
                ) {
                    Text(
                        text = row.name,
                        style = MaterialTheme.typography.bodyLarge,
                        fontWeight = FontWeight.SemiBold,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f, fill = false),
                    )
                    DeviceBadge(label = row.badge, paired = row.paired && !row.offline, offline = row.offline)
                }
                Text(
                    text = row.meta,
                    color = colors.contentMuted,
                    style = MaterialTheme.typography.bodyMedium,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
        if (row.onRemove != null) {
            PicooIconButton(onClick = onManage) {
                ReiconIcon(
                    icon = Reicon.MoreActions,
                    contentDescription = "管理 ${row.name}",
                    tint = colors.contentMuted,
                    modifier = Modifier.size(dimensions.iconStandard),
                )
            }
        }
    }
}

@Composable
private fun ManualConnectSheet(
    endpoint: String,
    connecting: Boolean,
    errorText: String?,
    onDismiss: () -> Unit,
    onConnect: (host: String, port: Int) -> Unit,
    onEndpointChange: (String) -> Unit,
) {
    var validationError by rememberSaveable { mutableStateOf<String?>(null) }
    val colors = PicooTheme.colors
    val dimensions = PicooTheme.dimensions
    PicooSheet(
        title = "输入电脑局域网地址",
        description = "仅在自动发现不可用时使用；连接后仍会核对双端短码。",
        onDismiss = onDismiss,
    ) {
        OutlinedTextField(
            value = endpoint,
            onValueChange = {
                onEndpointChange(it)
                validationError = null
            },
            modifier = Modifier.fillMaxWidth(),
            label = { Text("IP:端口") },
            placeholder = { Text("192.168.1.108:4433") },
            singleLine = true,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
        )
        (validationError ?: errorText)?.let { message ->
            Text(
                text = message,
                color = colors.statusDanger,
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(top = dimensions.space8),
            )
        }
        Spacer(modifier = Modifier.height(dimensions.space16))
        PicooPrimaryButton(
            text = if (connecting) "正在连接…" else "连接电脑",
            onClick = {
                val parts = endpoint.trim().split(":", limit = 2)
                val host = parts.firstOrNull()?.trim().orEmpty()
                val portText = parts.getOrNull(1)?.trim()
                val port = portText?.toIntOrNull() ?: 4433
                if (host.isBlank()) {
                    validationError = "请输入有效的局域网 IP 地址"
                } else if (portText != null && (portText.toIntOrNull() == null || port !in 1..65535)) {
                    validationError = "端口必须是 1 到 65535 之间的数字"
                } else {
                    onConnect(host, port)
                }
            },
            modifier = Modifier.fillMaxWidth(),
            enabled = !connecting,
        )
    }
}

@Composable
private fun EmptyDiscoveryHint(modifier: Modifier = Modifier) {
    val colors = PicooTheme.colors
    val dimensions = PicooTheme.dimensions
    Column(
        modifier = modifier
            .fillMaxWidth()
            .padding(vertical = dimensions.space24),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(dimensions.space8),
    ) {
        ReiconIcon(
            icon = Reicon.ReceiverDevice,
            contentDescription = null,
            tint = colors.contentMuted,
            modifier = Modifier.size(dimensions.iconHero),
        )
        Text(text = "未发现附近电脑", style = MaterialTheme.typography.titleMedium)
        Text(
            text = "确认电脑端 Picoo Camera 已打开，并与手机连接同一 Wi‑Fi。仍未找到时，可输入电脑端显示的 IP 地址直连。",
            color = colors.contentMuted,
            style = MaterialTheme.typography.bodyMedium,
        )
    }
}
