package com.picoo.camera.ui.screens

import androidx.compose.foundation.clickable
import androidx.compose.foundation.background
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Icon
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
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
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import com.picoo.camera.BuildConfig
import com.picoo.camera.jni.PicooNative
import com.picoo.camera.R
import com.picoo.camera.media.StreamResolution
import com.picoo.camera.pairing.TrustedDeviceList
import com.picoo.camera.ui.ResolutionSheetOptions
import com.picoo.camera.ui.components.PicooIconButton
import com.picoo.camera.ui.components.PicooSheet
import com.picoo.camera.ui.components.PicooSheetRow
import com.picoo.camera.ui.components.PicooSurfacePanel
import com.picoo.camera.ui.components.Reicon
import com.picoo.camera.ui.components.ReiconIcon
import com.picoo.camera.ui.theme.PicooTheme

/** REQ-PICOO-UI-012 / AC-M-SET-01..03 — native grouped settings. */
@OptIn(ExperimentalMaterial3Api::class)
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
    onSelectDefaultResolution: (String) -> Unit,
    modifier: Modifier = Modifier,
    pairedDevices: List<PicooNative.TrustedDevice> = emptyList(),
    errorText: String? = null,
    onRemovePaired: (PicooNative.TrustedDevice) -> Unit = {},
) {
    var showPairedSheet by rememberSaveable { mutableStateOf(false) }
    var showResolutionSheet by rememberSaveable { mutableStateOf(false) }
    var pendingRemoval by remember { mutableStateOf<PicooNative.TrustedDevice?>(null) }
    val colors = PicooTheme.colors
    val dimensions = PicooTheme.dimensions
    val permissionsReady = cameraGranted && nearbyWifiGranted && notificationsGranted

    Scaffold(
        modifier = modifier.fillMaxSize(),
        containerColor = colors.surfacePage,
        topBar = {
            TopAppBar(
                title = { Text("手机端设置", style = MaterialTheme.typography.titleLarge) },
                navigationIcon = {
                    PicooIconButton(
                        onClick = onBack,
                        contentDescription = "返回设备列表",
                    ) {
                        ReiconIcon(
                            icon = Reicon.NavigateBack,
                            contentDescription = null,
                            modifier = Modifier.size(dimensions.iconStandard),
                        )
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
                    .widthIn(max = dimensions.controlContentWidth),
                contentPadding = PaddingValues(
                    start = dimensions.screenGutter,
                    end = dimensions.screenGutter,
                    top = innerPadding.calculateTopPadding() + dimensions.space16,
                    bottom = innerPadding.calculateBottomPadding() + dimensions.space24,
                ),
                verticalArrangement = Arrangement.spacedBy(dimensions.sectionGap),
            ) {
                item {
                    SettingsSection(title = "连接") {
                        SettingsToggleRow(
                            title = "打开 App 自动直连",
                            description = "上次连接的电脑在线时自动连接",
                            checked = autoConnectEnabled,
                            onClick = onToggleAutoConnect,
                            leadingContent = {
                                SettingsIconContainer {
                                    ReiconIcon(
                                        icon = Reicon.Network,
                                        contentDescription = null,
                                        modifier = Modifier.size(dimensions.iconEmphasis),
                                    )
                                }
                            },
                        )
                        SettingsDivider()
                        SettingsValueRow(
                            title = "默认初始画质",
                            description = "新连接建立时的起步分辨率",
                            value = defaultResolutionLabel,
                            onClick = { showResolutionSheet = true },
                            leadingContent = {
                                SettingsIconContainer { QualityGlyph() }
                            },
                        )
                    }
                }
                item {
                    SettingsSection(title = "设备与权限") {
                        SettingsValueRow(
                            title = "已配对信任电脑",
                            description = "查看公钥指纹与撤销信任",
                            value = "$pairedDeviceCount 台",
                            onClick = { showPairedSheet = true },
                            leadingContent = {
                                SettingsIconContainer {
                                    ReiconIcon(
                                        icon = Reicon.SecureConnection,
                                        contentDescription = null,
                                        modifier = Modifier.size(dimensions.iconEmphasis),
                                    )
                                }
                            },
                        )
                        SettingsDivider()
                        SettingsValueRow(
                            title = "权限就绪状态",
                            description = "相机、附近 Wi‑Fi 与通知",
                            value = if (permissionsReady) "全部就绪" else "待检查",
                            valueColor = if (permissionsReady) colors.statusSuccess else colors.statusWarning,
                            onClick = onCheckPermissions,
                            leadingContent = {
                                SettingsIconContainer {
                                    ReiconIcon(
                                        icon = if (permissionsReady) {
                                            Reicon.SecureConnection
                                        } else {
                                            Reicon.Expired
                                        },
                                        contentDescription = null,
                                        modifier = Modifier.size(dimensions.iconEmphasis),
                                    )
                                }
                            },
                        )
                        if (!permissionsReady) {
                            SettingsDivider()
                            PermissionHint(
                                cameraGranted = cameraGranted,
                                nearbyWifiGranted = nearbyWifiGranted,
                                notificationsGranted = notificationsGranted,
                            )
                        }
                    }
                }
                item {
                    SettingsSection(title = "关于") {
                        SettingsValueRow(
                            title = "Picoo Camera",
                            description = "无线低延迟摄像头",
                            value = "v${BuildConfig.VERSION_NAME}",
                            onClick = null,
                            leadingContent = { PicooBrandMark() },
                        )
                    }
                }
            }
        }
    }

    if (showPairedSheet) {
        PairedDevicesSheet(
            devices = pairedDevices,
            errorText = errorText,
            onDismiss = { showPairedSheet = false },
            onRemove = { pendingRemoval = it },
            onFallback = {
                showPairedSheet = false
                onOpenPairedDevices()
            },
        )
    }
    if (showResolutionSheet) {
        DefaultResolutionSheet(
            selectedLabel = defaultResolutionLabel,
            onDismiss = { showResolutionSheet = false },
            onSelect = { label ->
                onSelectDefaultResolution(label)
                showResolutionSheet = false
            },
        )
    }
    pendingRemoval?.let { device ->
        TrustedDeviceRemovalDialog(
            deviceName = device.deviceName,
            onDismiss = { pendingRemoval = null },
            onConfirm = {
                pendingRemoval = null
                onRemovePaired(device)
            },
        )
    }
}

@Composable
private fun SettingsSection(
    title: String,
    modifier: Modifier = Modifier,
    content: @Composable ColumnScope.() -> Unit,
) {
    val colors = PicooTheme.colors
    val dimensions = PicooTheme.dimensions
    Column(modifier = modifier, verticalArrangement = Arrangement.spacedBy(dimensions.space12)) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(dimensions.space8),
        ) {
            Box(
                modifier = Modifier
                    .size(width = dimensions.space4, height = dimensions.space24)
                    .background(colors.actionHighlight, RoundedCornerShape(dimensions.space4)),
            )
            Text(
                text = title,
                color = colors.contentPrimary,
                style = MaterialTheme.typography.titleMedium,
            )
        }
        PicooSurfacePanel(
            modifier = Modifier.fillMaxWidth(),
        ) {
            Column { content() }
        }
    }
}

@Composable
private fun SettingsToggleRow(
    title: String,
    description: String,
    checked: Boolean,
    onClick: () -> Unit,
    leadingContent: @Composable () -> Unit,
) {
    val dimensions = PicooTheme.dimensions
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .toggleable(
                value = checked,
                role = Role.Switch,
                onValueChange = { onClick() },
            )
            .semantics(mergeDescendants = true) {}
            .defaultMinSize(minHeight = dimensions.touchTarget)
            .padding(horizontal = dimensions.space16, vertical = dimensions.space16),
        horizontalArrangement = Arrangement.spacedBy(dimensions.space12),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        leadingContent()
        SettingsText(title, description, Modifier.weight(1f))
        Switch(checked = checked, onCheckedChange = null)
    }
}

@Composable
private fun SettingsValueRow(
    title: String,
    description: String,
    value: String,
    onClick: (() -> Unit)?,
    valueColor: Color = PicooTheme.colors.contentMuted,
    leadingContent: @Composable () -> Unit,
) {
    val dimensions = PicooTheme.dimensions
    val colors = PicooTheme.colors
    val rowModifier = if (onClick != null) {
        Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .semantics { role = Role.Button }
    } else {
        Modifier.fillMaxWidth()
    }
    Row(
        modifier = rowModifier
            .defaultMinSize(minHeight = dimensions.touchTarget)
            .padding(
                horizontal = dimensions.space16,
                vertical = dimensions.space16,
            ),
        horizontalArrangement = Arrangement.spacedBy(dimensions.space12),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        leadingContent()
        SettingsText(title, description, Modifier.weight(1f))
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(dimensions.space4),
        ) {
            Text(
                text = value,
                color = valueColor,
                style = MaterialTheme.typography.bodyLarge,
                fontWeight = FontWeight.Medium,
            )
            if (onClick != null) {
                ReiconIcon(
                    icon = Reicon.NavigateBack,
                    contentDescription = null,
                    tint = colors.contentMuted,
                    modifier = Modifier
                        .size(dimensions.iconCompact)
                        .graphicsLayer(rotationZ = 180f),
                )
            }
        }
    }
}

@Composable
private fun SettingsIconContainer(
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit,
) {
    val colors = PicooTheme.colors
    val dimensions = PicooTheme.dimensions
    Surface(
        modifier = modifier.size(dimensions.settingsIconContainer),
        color = colors.surfaceAccent,
        contentColor = colors.actionHighlight,
        shape = RoundedCornerShape(dimensions.radiusIconContainer),
    ) {
        Box(contentAlignment = Alignment.Center) { content() }
    }
}

@Composable
private fun QualityGlyph(modifier: Modifier = Modifier) {
    Text(
        text = "HD",
        modifier = modifier,
        color = PicooTheme.colors.actionHighlight,
        style = MaterialTheme.typography.labelLarge,
        fontWeight = FontWeight.Bold,
    )
}

@Composable
private fun PicooBrandMark(modifier: Modifier = Modifier) {
    val colors = PicooTheme.colors
    val dimensions = PicooTheme.dimensions
    Box(
        modifier = modifier
            .size(dimensions.settingsIconContainer)
            .clip(RoundedCornerShape(dimensions.radiusIconContainer))
            .background(
                brush = Brush.linearGradient(
                    colors = listOf(colors.actionPrimary, colors.actionHighlight),
                ),
            ),
        contentAlignment = Alignment.Center,
    ) {
        Icon(
            painter = painterResource(R.drawable.ic_launcher_foreground),
            contentDescription = null,
            tint = colors.actionOnPrimary,
            modifier = Modifier.size(dimensions.settingsIconContainer),
        )
    }
}

@Composable
internal fun TrustedDeviceRemovalDialog(
    deviceName: String,
    onDismiss: () -> Unit,
    onConfirm: () -> Unit,
) {
    val colors = PicooTheme.colors
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("撤销对 $deviceName 的信任？") },
        text = { Text("下次连接必须重新在手机与电脑上核对 6 位短码。") },
        confirmButton = {
            TextButton(onClick = onConfirm) {
                Text("撤销信任", color = colors.statusDanger)
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text("取消") }
        },
    )
}

@Composable
private fun SettingsText(title: String, description: String, modifier: Modifier = Modifier) {
    val colors = PicooTheme.colors
    Column(modifier = modifier) {
        Text(
            text = title,
            style = MaterialTheme.typography.bodyLarge,
            fontWeight = FontWeight.SemiBold,
        )
        Text(
            text = description,
            color = colors.contentMuted,
            style = MaterialTheme.typography.bodyMedium,
        )
    }
}

@Composable
private fun SettingsDivider() {
    HorizontalDivider(
        modifier = Modifier.padding(
            start = PicooTheme.dimensions.space16 +
                PicooTheme.dimensions.settingsIconContainer +
                PicooTheme.dimensions.space12,
        ),
        color = PicooTheme.colors.borderDefault,
    )
}

@Composable
private fun PermissionHint(
    cameraGranted: Boolean,
    nearbyWifiGranted: Boolean,
    notificationsGranted: Boolean,
) {
    val dimensions = PicooTheme.dimensions
    Column(
        modifier = Modifier.padding(horizontal = dimensions.space16, vertical = dimensions.space12),
        verticalArrangement = Arrangement.spacedBy(dimensions.space4),
    ) {
        PermissionLine("相机", cameraGranted)
        PermissionLine("附近 Wi‑Fi 设备", nearbyWifiGranted)
        PermissionLine("通知（前台保活）", notificationsGranted)
    }
}

@Composable
private fun PermissionLine(label: String, granted: Boolean) {
    val colors = PicooTheme.colors
    Text(
        text = if (granted) "已就绪 · $label" else "待授权 · $label",
        color = if (granted) colors.statusSuccess else colors.statusWarning,
        style = MaterialTheme.typography.bodyMedium,
    )
}

@Composable
private fun PairedDevicesSheet(
    devices: List<PicooNative.TrustedDevice>,
    errorText: String?,
    onDismiss: () -> Unit,
    onRemove: (PicooNative.TrustedDevice) -> Unit,
    onFallback: () -> Unit,
) {
    val colors = PicooTheme.colors
    val dimensions = PicooTheme.dimensions
    PicooSheet(
        title = "已配对信任电脑",
        description = if (devices.isEmpty()) {
            "还没有已固定的电脑。首次配对成功后会出现在这里。"
        } else {
            "查看公钥短指纹，或撤销不再使用的电脑。"
        },
        onDismiss = onDismiss,
    ) {
        errorText?.let { message ->
            Text(
                text = message,
                color = colors.statusDanger,
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(dimensions.space8),
            )
        }
        devices.forEach { device ->
            PicooSheetRow(
                title = device.deviceName,
                subtitle = "公钥指纹 ${TrustedDeviceList.shortFingerprint(device.certificateFingerprint)} · 点按撤销",
                danger = true,
                onClick = { onRemove(device) },
            )
        }
        PicooSheetRow(title = "在设备列表中管理", onClick = onFallback)
        PicooSheetRow(title = "取消", onClick = onDismiss)
    }
}

@Composable
private fun DefaultResolutionSheet(
    selectedLabel: String,
    onDismiss: () -> Unit,
    onSelect: (String) -> Unit,
) {
    val selected = StreamResolution.fromLabel(selectedLabel)
    PicooSheet(
        title = "发送画质规格",
        description = "新连接建立时的起步分辨率。推流中仍可即时切换。",
        onDismiss = onDismiss,
    ) {
        ResolutionSheetOptions.all.forEach { option ->
            PicooSheetRow(
                title = option.title,
                subtitle = option.subtitle,
                selected = option.resolution == selected,
                onClick = { onSelect(option.resolution.label) },
            )
        }
        PicooSheetRow(title = "取消", onClick = onDismiss)
    }
}
