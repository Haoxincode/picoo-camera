package com.picoo.camera.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.style.TextAlign
import com.picoo.camera.discovery.DiscoveredReceiverRow
import com.picoo.camera.jni.PicooNative
import com.picoo.camera.pairing.TrustedDeviceList
import com.picoo.camera.ui.components.PicooGhostButton
import com.picoo.camera.ui.components.PicooButtonVariant
import com.picoo.camera.ui.components.PicooIconButton
import com.picoo.camera.ui.components.PicooSheet
import com.picoo.camera.ui.components.PicooSheetRow
import com.picoo.camera.ui.components.Reicon
import com.picoo.camera.ui.components.ReiconIcon
import com.picoo.camera.ui.theme.PicooTheme

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
    onRestartDiscovery: () -> Unit,
    onStopDiscovery: () -> Unit,
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
                        style = MaterialTheme.typography.titleLarge,
                    )
                },
                actions = {
                    WifiStatusPill(text = wifiPillText)
                    Spacer(modifier = Modifier.width(dimensions.space8))
                    Surface(
                        modifier = Modifier.padding(end = dimensions.space8),
                        color = colors.surfaceGroup,
                        shape = androidx.compose.foundation.shape.CircleShape,
                        shadowElevation = dimensions.space4,
                    ) {
                        PicooIconButton(
                            onClick = onOpenSettings,
                            contentDescription = "打开手机端设置",
                        ) {
                            ReiconIcon(
                                icon = Reicon.Settings,
                                contentDescription = null,
                                modifier = Modifier.size(dimensions.iconEmphasis),
                            )
                        }
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = colors.surfacePage),
            )
        },
    ) { innerPadding ->
        Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.TopCenter) {
            LazyColumn(
                modifier = Modifier
                    .fillMaxWidth()
                    .widthIn(max = dimensions.maxContentWidth),
                contentPadding = PaddingValues(
                    start = dimensions.screenGutter,
                    end = dimensions.screenGutter,
                    top = innerPadding.calculateTopPadding() + dimensions.heroTopSpacing,
                    bottom = innerPadding.calculateBottomPadding() + dimensions.space24,
                ),
                verticalArrangement = Arrangement.spacedBy(dimensions.sectionGap),
            ) {
                item {
                    Column(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalAlignment = Alignment.CenterHorizontally,
                        verticalArrangement = Arrangement.spacedBy(dimensions.space8),
                    ) {
                        Text(
                            text = "把手机变成无线摄像头",
                            style = MaterialTheme.typography.headlineLarge,
                            textAlign = TextAlign.Center,
                        )
                        Text(
                            text = "与电脑连接同一 Wi‑Fi，即可自动发现并连接。",
                            color = colors.contentMuted,
                            style = MaterialTheme.typography.bodyMedium,
                            textAlign = TextAlign.Center,
                        )
                    }
                }

                if (rows.isNotEmpty()) {
                    item {
                        DiscoveryStatusLine(searching = searching, hasDevices = true)
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
                    item {
                        PicooGhostButton(
                            text = "通过 IP 地址连接",
                            onClick = { manualSheetOpen = true },
                            modifier = Modifier.fillMaxWidth(),
                            variant = PicooButtonVariant.AccentOutline,
                            leadingContent = {
                                Text(
                                    text = "IP",
                                    style = MaterialTheme.typography.labelLarge,
                                    fontWeight = FontWeight.Bold,
                                )
                            },
                        )
                    }
                } else {
                    item {
                        DiscoveryHeroPanel(
                            searching = searching,
                            permissionRequired = !nearbyWifiGranted,
                            onPrimaryAction = if (searching) onStopDiscovery else onRestartDiscovery,
                            onManualConnect = { manualSheetOpen = true },
                            onCheckPermissions = onCheckPermissions,
                            onRequestPermission = onRequestNearbyWifi,
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
