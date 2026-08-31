package com.picoo.camera.ui.screens

import androidx.compose.foundation.clickable
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.disabled
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.material3.Text
import com.picoo.camera.ui.components.Reicon
import com.picoo.camera.ui.components.ReiconIcon
import com.picoo.camera.ui.theme.PicooCameraColors
import com.picoo.camera.ui.theme.PicooCameraDimensions
import com.picoo.camera.ui.theme.PicooCameraTypography
import com.picoo.camera.ui.theme.PicooFont
import com.picoo.camera.ui.theme.PicooTheme

@Composable
internal fun ConnectionHud(
    receiverName: String,
    linkQualityChip: String,
    bitrateMbps: String,
    resolutionLabel: String,
    packetLossLabel: String,
    thermalForced720: Boolean,
    enabled: Boolean,
    onToggleResolution: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val dimensions = PicooTheme.dimensions
    val latency = linkQualityChip.substringAfter(" · ", missingDelimiterValue = "--ms")
    val displayName = receiverName.ifBlank { "Picoo Camera" }
    Column(
        modifier = modifier
            .statusBarsPadding()
            .widthIn(max = dimensions.controlContentWidth)
            .fillMaxWidth()
            .padding(
                start = dimensions.space24,
                end = dimensions.space24,
                top = dimensions.space16,
            ),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Row(
            modifier = Modifier.semantics(mergeDescendants = true) {
                contentDescription = "$displayName 已连接"
                stateDescription = "$latency，$bitrateMbps，$packetLossLabel"
            },
            horizontalArrangement = Arrangement.spacedBy(dimensions.space8),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(
                modifier = Modifier
                    .size(PicooCameraDimensions.ConnectionDot)
                    .background(PicooCameraColors.Selected, CircleShape),
            )
            Text(
                text = "$displayName 已连接",
                color = PicooCameraColors.Content,
                style = PicooCameraTypography.HudTitle,
            )
        }
        Spacer(modifier = Modifier.height(dimensions.space4))
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(dimensions.space8),
        ) {
            TelemetryText(text = latency)
            TelemetrySeparator()
            TelemetryText(text = bitrateMbps)
            TelemetrySeparator()
            ResolutionMetric(
                resolutionLabel = resolutionLabel,
                thermalForced720 = thermalForced720,
                enabled = enabled,
                onClick = onToggleResolution,
            )
        }
    }
}

@Composable
internal fun TelemetryText(text: String) {
    Text(
        text = text,
        color = PicooCameraColors.ContentMuted,
        style = PicooCameraTypography.Telemetry.copy(fontFamily = PicooFont.Mono),
    )
}

@Composable
internal fun TelemetrySeparator() {
    Text(
        text = "•",
        color = PicooCameraColors.ContentSubtle,
        style = PicooCameraTypography.Telemetry,
    )
}

@Composable
internal fun ResolutionMetric(
    resolutionLabel: String,
    thermalForced720: Boolean,
    enabled: Boolean,
    onClick: () -> Unit,
) {
    val dimensions = PicooTheme.dimensions
    Row(
        modifier = Modifier
            .height(dimensions.touchTarget)
            .alpha(if (enabled) 1f else PicooCameraDimensions.DisabledAlpha)
            .semantics {
                contentDescription = "切换画质，当前 $resolutionLabel 30fps"
                stateDescription = if (thermalForced720) "设备偏热，已限制为 720p" else "可切换"
                role = Role.Button
                if (!enabled) disabled()
            }
            .clickable(enabled = enabled, role = Role.Button, onClick = onClick),
        horizontalArrangement = Arrangement.spacedBy(dimensions.space4),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (thermalForced720) {
            ReiconIcon(
                icon = Reicon.Overheat,
                contentDescription = null,
                tint = PicooCameraColors.Warning,
                modifier = Modifier.size(dimensions.iconCompact),
            )
        }
        Text(
            text = resolutionLabel.lowercase(),
            color = if (thermalForced720) PicooCameraColors.Warning else PicooCameraColors.ContentMuted,
            style = PicooCameraTypography.Telemetry.copy(fontFamily = PicooFont.Mono),
        )
        Text(
            text = "/",
            color = PicooCameraColors.Selected,
            style = PicooCameraTypography.Telemetry.copy(fontFamily = PicooFont.Mono),
        )
        Text(
            text = "30fps",
            color = PicooCameraColors.ContentMuted,
            style = PicooCameraTypography.Telemetry.copy(fontFamily = PicooFont.Mono),
        )
    }
}
