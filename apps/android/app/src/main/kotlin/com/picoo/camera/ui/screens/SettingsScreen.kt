package com.picoo.camera.ui.screens

import androidx.compose.foundation.clickable
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import com.picoo.camera.BuildConfig
import com.picoo.camera.jni.PicooNative
import com.picoo.camera.media.StreamResolution
import com.picoo.camera.pairing.TrustedDeviceList
import com.picoo.camera.ui.ResolutionSheetOptions
import com.picoo.camera.ui.components.PicooIconButton
import com.picoo.camera.ui.components.PicooSheet
import com.picoo.camera.ui.components.PicooSheetRow
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
                title = { Text("手机端设置", style = MaterialTheme.typography.titleMedium) },
                navigationIcon = {
                    PicooIconButton(onClick = onBack) {
                        ReiconIcon(
                            icon = Reicon.NavigateBack,
                            contentDescription = "返回设备列表",
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
                verticalArrangement = Arrangement.spacedBy(dimensions.space24),
            ) {
                item {
                    SettingsSection(title = "连接") {
                        SettingsToggleRow(
                            title = "打开 App 自动直连",
                            description = "上次连接的电脑在线时自动连接",
                            checked = autoConnectEnabled,
                            onClick = onToggleAutoConnect,
                        )
                        SettingsDivider()
                        SettingsValueRow(
                            title = "默认初始画质",
                            description = "新连接建立时的起步分辨率",
                            value = defaultResolutionLabel,
                            onClick = { showResolutionSheet = true },
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
                        )
                        SettingsDivider()
                        SettingsValueRow(
                            title = "权限就绪状态",
                            description = "相机、附近 Wi‑Fi 与通知",
                            value = if (permissionsReady) "全部就绪" else "待检查",
                            valueColor = if (permissionsReady) colors.statusSuccess else colors.statusWarning,
                            onClick = onCheckPermissions,
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
    Column(modifier = modifier, verticalArrangement = Arrangement.spacedBy(dimensions.space8)) {
        Text(
            text = title,
            color = colors.contentMuted,
            style = MaterialTheme.typography.labelLarge,
            modifier = Modifier.padding(horizontal = dimensions.space4),
        )
        Surface(
            modifier = Modifier.fillMaxWidth(),
            color = colors.surfaceGroup,
            shape = RoundedCornerShape(dimensions.radiusSurface),
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
            .padding(horizontal = dimensions.space16, vertical = dimensions.space12),
        horizontalArrangement = Arrangement.spacedBy(dimensions.space12),
        verticalAlignment = Alignment.CenterVertically,
    ) {
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
) {
    val dimensions = PicooTheme.dimensions
    val rowModifier = if (onClick != null) {
        Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .semantics { role = Role.Button }
    } else {
        Modifier.fillMaxWidth()
    }
    Row(
        modifier = rowModifier.padding(
            horizontal = dimensions.space16,
            vertical = dimensions.space12,
        ),
        horizontalArrangement = Arrangement.spacedBy(dimensions.space12),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        SettingsText(title, description, Modifier.weight(1f))
        Text(
            text = if (onClick == null) value else "$value  ›",
            color = valueColor,
            style = MaterialTheme.typography.bodyMedium,
            fontWeight = FontWeight.Medium,
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
        Text(text = title, style = MaterialTheme.typography.bodyLarge)
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
        modifier = Modifier.padding(start = PicooTheme.dimensions.space16),
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
