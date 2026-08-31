package com.picoo.camera.ui.screens

import android.animation.ValueAnimator
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.clickable
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
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
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.picoo.camera.R
import com.picoo.camera.discovery.DiscoveredReceiverRow
import com.picoo.camera.jni.PicooNative
import com.picoo.camera.pairing.TrustedDeviceList
import com.picoo.camera.ui.components.DeviceBadge
import com.picoo.camera.ui.components.PicooGhostButton
import com.picoo.camera.ui.components.PicooButtonVariant
import com.picoo.camera.ui.components.PicooIconButton
import com.picoo.camera.ui.components.PicooPrimaryButton
import com.picoo.camera.ui.components.PicooSheet
import com.picoo.camera.ui.components.PicooSheetRow
import com.picoo.camera.ui.components.PicooSurfacePanel
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
                            onPrimaryAction = if (searching) onStopDiscovery else onRestartDiscovery,
                            onManualConnect = { manualSheetOpen = true },
                            onCheckPermissions = onCheckPermissions,
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
private fun WifiStatusPill(text: String, modifier: Modifier = Modifier) {
    val colors = PicooTheme.colors
    val dimensions = PicooTheme.dimensions
    Row(
        modifier = modifier
            .background(colors.surfaceAccent, androidx.compose.foundation.shape.CircleShape)
            .padding(horizontal = dimensions.space12, vertical = dimensions.space8),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(dimensions.space8),
    ) {
        ReiconIcon(
            icon = Reicon.Network,
            contentDescription = null,
            tint = colors.actionHighlight,
            modifier = Modifier.size(dimensions.iconStandard),
        )
        Text(
            text = text,
            color = colors.actionHighlight,
            style = MaterialTheme.typography.labelLarge,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

@Composable
private fun DiscoveryStatusLine(
    searching: Boolean,
    hasDevices: Boolean,
    modifier: Modifier = Modifier,
) {
    val colors = PicooTheme.colors
    val dimensions = PicooTheme.dimensions
    Row(
        modifier = modifier
            .fillMaxWidth()
            .semantics { liveRegion = LiveRegionMode.Polite },
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
                searching -> "正在搜索同一 Wi‑Fi 下的电脑…"
                hasDevices -> "已找到可连接的电脑"
                else -> "暂未找到可连接的电脑"
            },
            color = colors.contentMuted,
            style = MaterialTheme.typography.bodyMedium,
        )
    }
}

@Composable
private fun DiscoveryHeroPanel(
    searching: Boolean,
    onPrimaryAction: () -> Unit,
    onManualConnect: () -> Unit,
    onCheckPermissions: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = PicooTheme.colors
    val dimensions = PicooTheme.dimensions
    PicooSurfacePanel(
        modifier = modifier
            .fillMaxWidth()
            .widthIn(max = dimensions.controlContentWidth),
    ) {
        Column(
            modifier = Modifier.padding(
                horizontal = dimensions.panelHorizontalPadding,
                vertical = dimensions.panelVerticalPadding,
            ),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(dimensions.space16),
        ) {
            DiscoveryDeviceIllustration(searching = searching)
            Column(
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(dimensions.space8),
                modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite },
            ) {
                Text(
                    text = if (searching) "正在查找附近电脑" else "未发现附近电脑",
                    style = MaterialTheme.typography.titleLarge,
                    textAlign = TextAlign.Center,
                )
                Text(
                    text = "请确认电脑端 Picoo Camera 已打开，\n并与手机连接到同一 Wi‑Fi。",
                    color = colors.contentMuted,
                    style = MaterialTheme.typography.bodyMedium,
                    textAlign = TextAlign.Center,
                )
            }
            if (searching) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(dimensions.space8),
                ) {
                    CircularProgressIndicator(
                        modifier = Modifier.size(dimensions.iconStandard),
                        color = colors.actionHighlight,
                        strokeWidth = dimensions.space2,
                    )
                    Text(
                        text = "正在搜索，请稍候…",
                        color = colors.actionHighlight,
                        style = MaterialTheme.typography.labelLarge,
                    )
                }
            }
            PicooPrimaryButton(
                text = if (searching) "停止搜索" else "重新搜索附近电脑",
                onClick = onPrimaryAction,
                modifier = Modifier.fillMaxWidth(),
                leadingContent = {
                    if (searching) {
                        StopSearchGlyph()
                    } else {
                        ReiconIcon(
                            icon = Reicon.RefreshDiscovery,
                            contentDescription = null,
                            modifier = Modifier.size(dimensions.iconEmphasis),
                        )
                    }
                },
            )
            PicooGhostButton(
                text = "通过 IP 地址连接",
                onClick = onManualConnect,
                modifier = Modifier.fillMaxWidth(),
                variant = PicooButtonVariant.AccentOutline,
                leadingContent = { IpAddressGlyph() },
            )
            Surface(
                onClick = onCheckPermissions,
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(min = dimensions.touchTarget)
                    .semantics { role = Role.Button },
                color = androidx.compose.ui.graphics.Color.Transparent,
                contentColor = colors.actionHighlight,
            ) {
                Row(
                    modifier = Modifier.padding(horizontal = dimensions.space8),
                    horizontalArrangement = Arrangement.Center,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        text = "找不到电脑？检查网络与发现权限",
                        style = MaterialTheme.typography.bodyMedium,
                        textAlign = TextAlign.Center,
                    )
                    ReiconIcon(
                        icon = Reicon.NavigateBack,
                        contentDescription = null,
                        modifier = Modifier
                            .size(dimensions.iconCompact)
                            .graphicsLayer(rotationZ = 180f),
                    )
                }
            }
        }
    }
}

@Composable
private fun StopSearchGlyph(modifier: Modifier = Modifier) {
    val dimensions = PicooTheme.dimensions
    val colors = PicooTheme.colors
    Box(
        modifier = modifier
            .size(dimensions.iconEmphasis)
            .border(
                dimensions.space2,
                colors.actionOnPrimary,
                androidx.compose.foundation.shape.CircleShape,
            ),
        contentAlignment = Alignment.Center,
    ) {
        ReiconIcon(
            icon = Reicon.Rejected,
            contentDescription = null,
            modifier = Modifier.size(dimensions.iconCompact),
        )
    }
}

@Composable
private fun DiscoveryDeviceIllustration(
    searching: Boolean,
    modifier: Modifier = Modifier,
) {
    val colors = PicooTheme.colors
    val dimensions = PicooTheme.dimensions
    val darkTheme = isSystemInDarkTheme()
    Box(
        modifier = modifier
            .fillMaxWidth()
            .height(dimensions.discoveryIllustrationHeight),
        contentAlignment = Alignment.Center,
    ) {
        Box(
            modifier = Modifier
                .size(dimensions.discoveryIllustrationHeight)
                .background(colors.surfaceAccent, androidx.compose.foundation.shape.CircleShape),
        )
        if (searching && ValueAnimator.areAnimatorsEnabled()) {
            AnimatedDiscoveryOrbit()
        } else {
            StaticDiscoveryOrbit()
        }
        Image(
            painter = painterResource(
                if (darkTheme) R.drawable.picoo_macbook_pro_dark
                else R.drawable.picoo_macbook_pro_light,
            ),
            contentDescription = null,
            contentScale = ContentScale.Fit,
            modifier = Modifier
                .align(Alignment.BottomCenter)
                .width(dimensions.discoveryDeviceWidth)
                .aspectRatio(5f / 3f),
        )
        ReiconIcon(
            icon = Reicon.Network,
            contentDescription = null,
            tint = colors.actionHighlight,
            modifier = Modifier
                .align(Alignment.TopCenter)
                .padding(top = dimensions.space24)
                .size(dimensions.iconHero),
        )
    }
}

@Composable
private fun AnimatedDiscoveryOrbit(modifier: Modifier = Modifier) {
    val dimensions = PicooTheme.dimensions
    val transition = rememberInfiniteTransition(label = "receiverDiscovery")
    val rotation by transition.animateFloat(
        initialValue = 0f,
        targetValue = 360f,
        animationSpec = infiniteRepeatable(
            animation = tween(
                durationMillis = PicooTheme.motion.deliberateMillis * 4,
                easing = LinearEasing,
            ),
        ),
        label = "receiverDiscoveryOrbit",
    )
    DiscoveryOrbit(
        modifier = modifier.graphicsLayer(rotationZ = rotation),
        showArc = true,
    )
}

@Composable
private fun StaticDiscoveryOrbit(modifier: Modifier = Modifier) {
    DiscoveryOrbit(modifier = modifier, showArc = false)
}

@Composable
private fun DiscoveryOrbit(
    showArc: Boolean,
    modifier: Modifier = Modifier,
) {
    val colors = PicooTheme.colors
    val dimensions = PicooTheme.dimensions
    Canvas(
        modifier = modifier.size(dimensions.discoveryIllustrationHeight - dimensions.space16),
    ) {
        val stroke = dimensions.space2.toPx()
        drawCircle(
            color = colors.borderAccent,
            style = Stroke(width = stroke),
        )
        drawCircle(
            color = colors.borderAccent.copy(alpha = 0.7f),
            radius = size.minDimension / 2 - dimensions.space24.toPx(),
            style = Stroke(width = stroke),
        )
        if (showArc) {
            drawArc(
                color = colors.actionHighlight.copy(alpha = 0.42f),
                startAngle = -32f,
                sweepAngle = 84f,
                useCenter = false,
                style = Stroke(width = dimensions.space8.toPx(), cap = StrokeCap.Round),
            )
        }
    }
}

@Composable
private fun IpAddressGlyph(modifier: Modifier = Modifier) {
    val colors = PicooTheme.colors
    val dimensions = PicooTheme.dimensions
    Box(
        modifier = modifier
            .size(dimensions.iconEmphasis)
            .border(
                width = dimensions.space2 / 2,
                color = colors.actionHighlight,
                shape = RoundedCornerShape(dimensions.radiusControl),
            ),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = "IP",
            style = MaterialTheme.typography.labelSmall,
            fontWeight = FontWeight.Bold,
        )
    }
}
