package com.picoo.camera.ui.components

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.IconButton
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.scale
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import com.picoo.camera.ui.theme.PicooColors
import com.picoo.camera.ui.theme.PicooFont

@Composable
fun PicooPrimaryButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
) {
    Button(
        onClick = onClick,
        modifier = modifier
            .clip(RoundedCornerShape(14.dp))
            .background(
                brush = Brush.linearGradient(
                    colors = listOf(
                        PicooColors.Accent,
                        PicooColors.AccentHover,
                        PicooColors.Accent2,
                    ),
                ),
            ),
        enabled = enabled,
        shape = RoundedCornerShape(14.dp),
        contentPadding = PaddingValues(vertical = 13.dp, horizontal = 16.dp),
        elevation = ButtonDefaults.buttonElevation(defaultElevation = 0.dp),
        colors = ButtonDefaults.buttonColors(
            containerColor = Color.Transparent,
            disabledContainerColor = PicooColors.Panel2,
            contentColor = Color(0xFFEFF6FF),
        ),
    ) {
        Text(
            text = text,
            fontFamily = PicooFont.Body,
            fontWeight = FontWeight.Bold,
            fontSize = 15.sp,
        )
    }
}

@Composable
fun PicooGhostButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    primary: Boolean = false,
    small: Boolean = false,
) {
    if (primary) {
        PicooPrimaryButton(text = text, onClick = onClick, modifier = modifier)
    } else {
        OutlinedButton(
            onClick = onClick,
            modifier = modifier.clip(RoundedCornerShape(if (small) 10.dp else 14.dp)),
            shape = RoundedCornerShape(if (small) 10.dp else 14.dp),
            contentPadding = PaddingValues(
                vertical = if (small) 8.dp else 13.dp,
                horizontal = if (small) 12.dp else 16.dp,
            ),
            colors = ButtonDefaults.outlinedButtonColors(
                containerColor = Color.Transparent,
                contentColor = PicooColors.Text,
            ),
            border = androidx.compose.foundation.BorderStroke(1.dp, PicooColors.Line),
        ) {
            Text(
                text = text,
                fontWeight = FontWeight.Bold,
                fontSize = if (small) 13.sp else 15.sp,
            )
        }
    }
}

@Composable
fun PicooDangerButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    armed: Boolean = false,
) {
    val bg = if (armed) PicooColors.Danger else PicooColors.DangerBg
    val fg = if (armed) Color.White else PicooColors.DangerText
    val border = if (armed) Color.Transparent else Color(0x47FF5C6C)
    OutlinedButton(
        onClick = onClick,
        modifier = modifier
            .clip(RoundedCornerShape(14.dp))
            .background(bg),
        shape = RoundedCornerShape(14.dp),
        contentPadding = PaddingValues(vertical = 13.dp, horizontal = 16.dp),
        colors = ButtonDefaults.outlinedButtonColors(contentColor = fg),
        border = androidx.compose.foundation.BorderStroke(1.dp, border),
    ) {
        Text(text = text, fontWeight = FontWeight.Bold, fontSize = 15.sp)
    }
}

@Composable
fun PicooIconButton(
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    content: @Composable () -> Unit,
) {
    IconButton(
        onClick = onClick,
        modifier = modifier
            .size(32.dp)
            .clip(RoundedCornerShape(10.dp))
            .border(1.dp, PicooColors.Line, RoundedCornerShape(10.dp)),
    ) {
        content()
    }
}

@Composable
fun PicooPill(
    text: String,
    modifier: Modifier = Modifier,
    warn: Boolean = false,
    neutral: Boolean = false,
) {
    val (bg, fg, border) = when {
        warn -> Triple(Color(0x1FF0C14A), PicooColors.Warn, Color(0x47F0C14A))
        neutral -> Triple(Color(0x0FFFFFFF), PicooColors.Muted, PicooColors.Line)
        else -> Triple(Color(0x1F3ECF8E), PicooColors.Ready, Color(0x403ECF8E))
    }
    Text(
        text = text,
        modifier = modifier
            .background(bg, RoundedCornerShape(999.dp))
            .border(1.dp, border, RoundedCornerShape(999.dp))
            .padding(horizontal = 9.dp, vertical = 5.dp),
        color = fg,
        fontSize = 11.sp,
        fontWeight = FontWeight.SemiBold,
    )
}

@Composable
fun DeviceBadge(label: String, paired: Boolean, offline: Boolean = false) {
    val fg = when {
        offline -> PicooColors.Muted
        paired -> PicooColors.Accent2
        else -> PicooColors.Ready
    }
    val bg = when {
        offline -> Color(0x0DFFFFFF)
        paired -> PicooColors.Accent2.copy(alpha = 0.12f)
        else -> Color(0x1F3ECF8E)
    }
    val border = when {
        offline -> PicooColors.Line
        paired -> PicooColors.Accent2.copy(alpha = 0.28f)
        else -> Color(0x383ECF8E)
    }
    Text(
        text = label,
        modifier = Modifier
            .background(bg, RoundedCornerShape(6.dp))
            .border(1.dp, border, RoundedCornerShape(6.dp))
            .padding(horizontal = 6.dp, vertical = 3.dp),
        color = fg,
        fontSize = 10.sp,
        fontWeight = FontWeight.Bold,
        letterSpacing = 0.3.sp,
    )
}

@Composable
fun DiscoveryPulseDot(searching: Boolean) {
    if (!searching) {
        Box(
            modifier = Modifier
                .size(8.dp)
                .clip(RoundedCornerShape(50))
                .background(PicooColors.Ready),
        )
        return
    }
    val transition = rememberInfiniteTransition(label = "pulse")
    val scale by transition.animateFloat(
        initialValue = 0.85f,
        targetValue = if (searching) 1.15f else 1f,
        animationSpec = infiniteRepeatable(
            animation = tween(1200, easing = LinearEasing),
            repeatMode = RepeatMode.Reverse,
        ),
        label = "pulseScale",
    )
    val alpha by transition.animateFloat(
        initialValue = if (searching) 0.35f else 1f,
        targetValue = 1f,
        animationSpec = infiniteRepeatable(
            animation = tween(1200, easing = LinearEasing),
            repeatMode = RepeatMode.Reverse,
        ),
        label = "pulseAlpha",
    )
    Box(
        modifier = Modifier
            .size(8.dp)
            .scale(scale)
            .clip(RoundedCornerShape(50))
            .background(
                if (searching) PicooColors.Accent.copy(alpha = alpha) else PicooColors.Ready,
            ),
    )
}

/** @deprecated use [DeviceBadge] */
@Composable
fun ReadinessBadge(label: String, paired: Boolean, offline: Boolean = false) {
    DeviceBadge(label = label, paired = paired, offline = offline)
}

/** 原型 `m-sheet`：底栏管理面板（AC-M-DISC-03 / AC-M-SET-02）。 */
@Composable
fun PicooSheet(
    title: String,
    description: String? = null,
    onDismiss: () -> Unit,
    content: @Composable ColumnScope.() -> Unit,
) {
    Dialog(
        onDismissRequest = onDismiss,
        properties = DialogProperties(usePlatformDefaultWidth = false),
    ) {
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(Color(0x99000000))
                .clickable(onClick = onDismiss),
        ) {
            Column(
                modifier = Modifier
                    .align(Alignment.BottomCenter)
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(topStart = 20.dp, topEnd = 20.dp))
                    .background(PicooColors.Panel3)
                    .border(
                        1.dp,
                        PicooColors.Line,
                        RoundedCornerShape(topStart = 20.dp, topEnd = 20.dp),
                    )
                    .clickable(enabled = false) {}
                    .padding(horizontal = 18.dp, vertical = 16.dp),
            ) {
                Text(
                    text = title,
                    color = PicooColors.Text,
                    fontFamily = PicooFont.Display,
                    fontSize = 18.sp,
                    fontWeight = FontWeight.Bold,
                )
                if (description != null) {
                    Text(
                        text = description,
                        color = PicooColors.Muted,
                        fontSize = 13.sp,
                        lineHeight = 18.sp,
                        modifier = Modifier.padding(top = 4.dp, bottom = 8.dp),
                    )
                } else {
                    Spacer(modifier = Modifier.height(8.dp))
                }
                content()
            }
        }
    }
}

@Composable
fun PicooSheetRow(
    title: String,
    subtitle: String? = null,
    selected: Boolean = false,
    danger: Boolean = false,
    onClick: () -> Unit,
) {
    val border = when {
        danger -> Color(0x47FF5C6C)
        selected -> PicooColors.Accent2.copy(alpha = 0.55f)
        else -> PicooColors.Line
    }
    val bg = when {
        selected -> PicooColors.Accent2.copy(alpha = 0.10f)
        else -> PicooColors.Panel2
    }
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(bottom = 8.dp)
            .clip(RoundedCornerShape(12.dp))
            .background(bg)
            .border(1.dp, border, RoundedCornerShape(12.dp))
            .clickable(onClick = onClick)
            .padding(horizontal = 14.dp, vertical = 12.dp),
    ) {
        Text(
            text = title,
            color = if (danger) PicooColors.DangerText else PicooColors.Text,
            fontSize = 15.sp,
            fontWeight = FontWeight.SemiBold,
        )
        if (subtitle != null) {
            Text(
                text = subtitle,
                color = if (danger) PicooColors.DangerText.copy(alpha = 0.85f) else PicooColors.Muted,
                fontSize = 12.sp,
                modifier = Modifier.padding(top = 3.dp),
            )
        }
    }
}
