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
import androidx.compose.foundation.layout.heightIn
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
import com.picoo.camera.ui.components.PicooIconButton
import com.picoo.camera.ui.components.PicooVisualContext
import com.picoo.camera.ui.theme.PicooCameraColors
import com.picoo.camera.ui.theme.PicooCameraDimensions
import com.picoo.camera.ui.theme.PicooCameraTypography
import com.picoo.camera.ui.theme.PicooFont
import com.picoo.camera.ui.theme.PicooTheme

@Composable
internal fun ConnectionHud(
    linkQualityChip: String,
    bitrateMbps: String,
    sourceLabel: String,
    packetLossLabel: String,
    thermalLimited: Boolean,
    enabled: Boolean,
    connected: Boolean,
    title: String,
    detail: String,
    onChooseSourceFormat: () -> Unit,
    onConnectionClick: () -> Unit,
    onOpenSettings: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val dimensions = PicooTheme.dimensions
    val latency = linkQualityChip.substringAfter(" · ", missingDelimiterValue = "--ms")
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
    Box(modifier = Modifier.fillMaxWidth()) {
            Row(
                modifier = Modifier
                    .align(Alignment.Center)
                    .heightIn(min = dimensions.touchTarget)
                    .semantics(mergeDescendants = true) {
                        contentDescription = title
                        stateDescription = if (connected) {
                            "$latency，$bitrateMbps，$packetLossLabel"
                        } else {
                            detail
                        }
                        role = Role.Button
                    }
                    .clickable(enabled = enabled, role = Role.Button, onClick = onConnectionClick),
                horizontalArrangement = Arrangement.spacedBy(dimensions.space8),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Box(
                    modifier = Modifier
                        .size(PicooCameraDimensions.ConnectionDot)
                        .background(
                            if (connected) PicooCameraColors.Selected else PicooCameraColors.ContentMuted,
                            CircleShape,
                        ),
                )
                Text(
                    text = title,
                    color = PicooCameraColors.Content,
                    style = PicooCameraTypography.HudTitle,
                )
            }
            PicooIconButton(
                onClick = onOpenSettings,
                contentDescription = "打开手机端设置",
                context = PicooVisualContext.Camera,
                modifier = Modifier.align(Alignment.CenterEnd),
            ) {
                ReiconIcon(
                    icon = Reicon.Settings,
                    contentDescription = null,
                    modifier = Modifier.size(dimensions.iconStandard),
                )
            }
        }
        if (connected) {
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
                    sourceLabel = sourceLabel,
                    thermalLimited = thermalLimited,
                    enabled = enabled,
                    onClick = onChooseSourceFormat,
                )
            }
        } else {
            Text(
                text = detail,
                color = PicooCameraColors.ContentMuted,
                style = PicooCameraTypography.Telemetry.copy(fontFamily = PicooFont.Mono),
                modifier = Modifier.padding(top = dimensions.space4),
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
    sourceLabel: String,
    thermalLimited: Boolean,
    enabled: Boolean,
    onClick: () -> Unit,
) {
    val dimensions = PicooTheme.dimensions
    Row(
        modifier = Modifier
            .height(dimensions.touchTarget)
            .alpha(if (enabled) 1f else PicooCameraDimensions.DisabledAlpha)
            .semantics {
                contentDescription = "切换画质，当前 $sourceLabel"
                stateDescription = if (thermalLimited) "设备偏热，可手动切换" else "可切换"
                role = Role.Button
                if (!enabled) disabled()
            }
            .clickable(enabled = enabled, role = Role.Button, onClick = onClick),
        horizontalArrangement = Arrangement.spacedBy(dimensions.space4),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (thermalLimited) {
            ReiconIcon(
                icon = Reicon.Overheat,
                contentDescription = null,
                tint = PicooCameraColors.Warning,
                modifier = Modifier.size(dimensions.iconCompact),
            )
        }
        Text(
            text = sourceLabel,
            color = if (thermalLimited) PicooCameraColors.Warning else PicooCameraColors.ContentMuted,
            style = PicooCameraTypography.Telemetry.copy(fontFamily = PicooFont.Mono),
        )
    }
}
