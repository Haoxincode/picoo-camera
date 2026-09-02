package com.picoo.camera.ui.screens

import androidx.compose.foundation.clickable
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.picoo.camera.ui.ManualEndpointDraft
import com.picoo.camera.ui.components.DeviceBadge
import com.picoo.camera.ui.components.PicooIconButton
import com.picoo.camera.ui.components.PicooPrimaryButton
import com.picoo.camera.ui.components.PicooSheet
import com.picoo.camera.ui.components.Reicon
import com.picoo.camera.ui.components.ReiconIcon
import com.picoo.camera.ui.theme.PicooTheme

internal data class DeviceRowModel(
    val name: String,
    val meta: String,
    val badge: String,
    val paired: Boolean,
    val offline: Boolean,
    val fingerprint: String?,
    val onClick: () -> Unit,
    val onRemove: (() -> Unit)?,
)

@Composable
internal fun DeviceRow(
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
internal fun ManualConnectSheet(
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
    val focusRequesters = List(5) { remember { FocusRequester() } }
    val focusManager = LocalFocusManager.current
    val draft = ManualEndpointDraft.from(endpoint)
    PicooSheet(
        title = "输入电脑局域网地址",
        description = "仅在自动发现不可用时使用；连接后仍会核对双端短码。",
        onDismiss = onDismiss,
    ) {
        Text(
            text = "IP 地址",
            color = colors.contentMuted,
            style = MaterialTheme.typography.labelMedium,
        )
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(top = dimensions.space4),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(2.dp),
        ) {
            draft.octets.forEachIndexed { index, octet ->
                EndpointNumberField(
                    value = octet,
                    placeholder = if (index == 0) "192" else if (index == 1) "168" else "1",
                    contentDescription = "IP 地址第 ${index + 1} 段",
                    focusRequester = focusRequesters[index],
                    imeAction = androidx.compose.ui.text.input.ImeAction.Next,
                    onNext = { focusRequesters[index + 1].requestFocus() },
                    onValueChange = { value ->
                        val pasted = ManualEndpointDraft.fromPastedText(value)
                        val updated = pasted ?: draft.updateOctet(index, value)
                        onEndpointChange(updated.asText())
                        validationError = null
                        if (pasted != null) {
                            focusRequesters.last().requestFocus()
                        } else if (ManualEndpointDraft.shouldAdvanceOctet(value)) {
                            focusRequesters[index + 1].requestFocus()
                        }
                    },
                    modifier = Modifier.weight(1f),
                )
                if (index != draft.octets.lastIndex) {
                    Text(".", style = MaterialTheme.typography.titleMedium)
                }
            }
            Text(":", style = MaterialTheme.typography.titleMedium)
            EndpointNumberField(
                value = draft.port,
                placeholder = ManualEndpointDraft.DEFAULT_PORT,
                contentDescription = "端口",
                focusRequester = focusRequesters.last(),
                imeAction = androidx.compose.ui.text.input.ImeAction.Done,
                onNext = { focusManager.clearFocus() },
                onValueChange = { value ->
                    onEndpointChange(draft.updatePort(value).asText())
                    validationError = null
                },
                modifier = Modifier.weight(1.35f),
            )
        }
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
                val validated = draft.validatedEndpoint()
                if (validated == null) {
                    validationError = "请完整输入有效的 IP 地址和端口"
                } else {
                    onConnect(validated.host, validated.port)
                }
            },
            modifier = Modifier.fillMaxWidth(),
            enabled = !connecting,
        )
    }
}

@Composable
private fun EndpointNumberField(
    value: String,
    placeholder: String,
    contentDescription: String,
    focusRequester: FocusRequester,
    imeAction: androidx.compose.ui.text.input.ImeAction,
    onNext: () -> Unit,
    onValueChange: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = PicooTheme.colors
    val dimensions = PicooTheme.dimensions
    val interactionSource = remember { MutableInteractionSource() }
    val focused by interactionSource.collectIsFocusedAsState()
    val shape = RoundedCornerShape(dimensions.radiusControl)
    BasicTextField(
        value = value,
        onValueChange = onValueChange,
        modifier = modifier
            .defaultMinSize(minWidth = 0.dp)
            .heightIn(min = dimensions.touchTarget)
            .border(
                width = 1.dp,
                color = if (focused) colors.actionHighlight else colors.borderDefault,
                shape = shape,
            )
            .padding(horizontal = dimensions.space2)
            .focusRequester(focusRequester)
            .semantics { this.contentDescription = contentDescription },
        singleLine = true,
        textStyle = MaterialTheme.typography.bodyMedium.copy(
            color = colors.contentPrimary,
            textAlign = TextAlign.Center,
        ),
        cursorBrush = SolidColor(colors.actionHighlight),
        interactionSource = interactionSource,
        keyboardOptions = KeyboardOptions(
            keyboardType = KeyboardType.Number,
            imeAction = imeAction,
        ),
        keyboardActions = KeyboardActions(
            onNext = { onNext() },
            onDone = { onNext() },
        ),
        decorationBox = { innerTextField ->
            Box(
                modifier = Modifier.fillMaxWidth(),
                contentAlignment = Alignment.Center,
            ) {
                if (value.isEmpty()) {
                    Text(
                        text = placeholder,
                        color = colors.contentMuted,
                        style = MaterialTheme.typography.bodySmall,
                        maxLines = 1,
                    )
                }
                innerTextField()
            }
        },
    )
}

@Composable
internal fun WifiStatusPill(text: String, modifier: Modifier = Modifier) {
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
internal fun DiscoveryStatusLine(
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
