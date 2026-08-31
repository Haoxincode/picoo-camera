package com.picoo.camera.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.disabled
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.material3.Text
import com.picoo.camera.ui.components.Reicon
import com.picoo.camera.ui.components.ReiconIcon
import com.picoo.camera.ui.theme.PicooCameraColors
import com.picoo.camera.ui.theme.PicooCameraDimensions
import com.picoo.camera.ui.theme.PicooCameraTypography
import com.picoo.camera.ui.theme.PicooTheme

@Composable
internal fun CameraControlDock(
    exposureEv: Int,
    evSupported: Boolean,
    localPreviewMirrored: Boolean,
    uiLocked: Boolean,
    disconnectArmed: Boolean,
    flipRotation: Float,
    thermalForced720: Boolean,
    powerHint: String,
    onCycleExposure: () -> Unit,
    onToggleMirror: () -> Unit,
    onToggleLock: () -> Unit,
    onDisconnect: () -> Unit,
    onFlipCamera: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val dimensions = PicooTheme.dimensions
    Column(
        modifier = modifier
            .fillMaxWidth()
            .background(
                Brush.verticalGradient(
                    colors = listOf(
                        PicooCameraColors.Surface.copy(alpha = 0f),
                        PicooCameraColors.BottomScrim,
                    ),
                ),
            )
            .navigationBarsPadding()
            .padding(
                start = dimensions.space24,
                end = dimensions.space24,
                top = dimensions.space32,
                bottom = dimensions.space16,
            ),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Column(
            modifier = Modifier
                .widthIn(max = dimensions.controlContentWidth)
                .fillMaxWidth(),
        ) {
            if (thermalForced720) {
                ThermalBanner()
                Spacer(modifier = Modifier.height(dimensions.space8))
            } else if (powerHint.isNotEmpty()) {
                PowerHint(text = powerHint)
                Spacer(modifier = Modifier.height(dimensions.space8))
            }

            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                DockControlButton(
                    icon = Reicon.Exposure,
                    label = exposureControlLabel(exposureEv),
                    contentDescription = "调节亮度",
                    stateDescription = exposureStateDescription(exposureEv, evSupported),
                    selected = exposureEv != 0,
                    enabled = !uiLocked && evSupported,
                    onClick = onCycleExposure,
                    modifier = Modifier.weight(1f),
                )
                DockDivider()
                DockControlButton(
                    icon = Reicon.Mirror,
                    label = "镜像",
                    contentDescription = "切换本机预览镜像",
                    stateDescription = if (localPreviewMirrored) "已开启" else "已关闭",
                    selected = localPreviewMirrored,
                    enabled = !uiLocked,
                    onClick = onToggleMirror,
                    modifier = Modifier.weight(1f),
                )
                DockDivider()
                DockControlButton(
                    icon = if (uiLocked) Reicon.InteractionLock else Reicon.InteractionUnlock,
                    label = if (uiLocked) "已锁定" else "锁定",
                    contentDescription = "防误触锁定",
                    stateDescription = if (uiLocked) "已锁定，再次点击解锁" else "未锁定",
                    selected = uiLocked,
                    enabled = true,
                    onClick = onToggleLock,
                    modifier = Modifier.weight(1f),
                )
            }

            Spacer(modifier = Modifier.height(dimensions.space8))
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .height(PicooCameraDimensions.DockHairline)
                    .background(PicooCameraColors.ControlBorderSubtle),
            )
            if (disconnectArmed) {
                Text(
                    text = "再次点击确认断开",
                    color = PicooCameraColors.DangerContent,
                    style = PicooCameraTypography.GuideHint,
                    textAlign = TextAlign.Center,
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(top = dimensions.space8),
                )
            }
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                DockActionButton(
                    icon = Reicon.Disconnect,
                    contentDescription = "断开连接",
                    stateDescription = if (disconnectArmed) "等待再次确认" else "未确认",
                    tint = if (disconnectArmed) {
                        PicooCameraColors.DangerEmphasis
                    } else {
                        PicooCameraColors.Danger
                    },
                    enabled = !uiLocked,
                    onClick = onDisconnect,
                    modifier = Modifier.weight(1f),
                )
                DockDivider()
                DockActionButton(
                    icon = Reicon.SwitchCamera,
                    contentDescription = "切换前后摄像头",
                    stateDescription = "可用",
                    tint = PicooCameraColors.Content,
                    enabled = !uiLocked,
                    iconRotation = flipRotation,
                    onClick = onFlipCamera,
                    modifier = Modifier.weight(1f),
                )
            }
        }
    }
}

@Composable
internal fun DockControlButton(
    icon: Reicon,
    label: String,
    contentDescription: String,
    stateDescription: String,
    selected: Boolean,
    enabled: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val dimensions = PicooTheme.dimensions
    val tint = if (selected) PicooCameraColors.Selected else PicooCameraColors.Content
    Column(
        modifier = modifier
            .height(PicooCameraDimensions.DockControlHeight)
            .alpha(if (enabled) 1f else PicooCameraDimensions.DisabledAlpha)
            .semantics {
                this.contentDescription = contentDescription
                this.stateDescription = stateDescription
                this.selected = selected
                role = Role.Button
                if (!enabled) disabled()
            }
            .clickable(enabled = enabled, role = Role.Button, onClick = onClick),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        ReiconIcon(
            icon = icon,
            contentDescription = null,
            tint = tint,
            modifier = Modifier.size(dimensions.iconEmphasis),
        )
        Spacer(modifier = Modifier.height(dimensions.space4))
        Text(
            text = label,
            color = tint,
            style = PicooCameraTypography.ControlLabel,
            maxLines = 1,
        )
    }
}

@Composable
internal fun DockActionButton(
    icon: Reicon,
    contentDescription: String,
    stateDescription: String,
    tint: Color,
    enabled: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    iconRotation: Float = 0f,
) {
    val dimensions = PicooTheme.dimensions
    Box(
        modifier = modifier
            .height(PicooCameraDimensions.DockActionHeight)
            .alpha(if (enabled) 1f else PicooCameraDimensions.DisabledAlpha)
            .semantics {
                this.contentDescription = contentDescription
                this.stateDescription = stateDescription
                role = Role.Button
                if (!enabled) disabled()
            }
            .clickable(enabled = enabled, role = Role.Button, onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        ReiconIcon(
            icon = icon,
            contentDescription = null,
            tint = tint,
            modifier = Modifier
                .size(dimensions.iconEmphasis)
                .rotate(iconRotation),
        )
    }
}

@Composable
internal fun DockDivider() {
    Box(
        modifier = Modifier
            .width(PicooCameraDimensions.DockHairline)
            .height(PicooCameraDimensions.DockDividerHeight)
            .background(PicooCameraColors.ControlBorder),
    )
}

internal fun exposureControlLabel(exposureEv: Int): String = when (exposureEv) {
    0 -> "亮度"
    1 -> "提亮 +1"
    2 -> "提亮 +2"
    -1 -> "压暗 -1"
    -2 -> "压暗 -2"
    else -> "EV $exposureEv"
}

internal fun exposureStateDescription(exposureEv: Int, supported: Boolean): String = when {
    !supported -> "当前设备不支持曝光补偿"
    exposureEv == 0 -> "自动测光"
    else -> exposureControlLabel(exposureEv)
}
