package com.picoo.camera.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.picoo.camera.ui.theme.PicooColors

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
            .fillMaxWidth()
            .background(
                brush = Brush.linearGradient(
                    colors = listOf(
                        PicooColors.Accent,
                        Color(0xFFFF864F),
                        PicooColors.Accent2,
                    ),
                ),
                shape = RoundedCornerShape(16.dp),
            ),
        enabled = enabled,
        shape = RoundedCornerShape(16.dp),
        contentPadding = PaddingValues(vertical = 14.dp, horizontal = 16.dp),
        colors = ButtonDefaults.buttonColors(
            containerColor = Color.Transparent,
            disabledContainerColor = PicooColors.Panel2,
            contentColor = Color(0xFF1A0D08),
        ),
    ) {
        Text(text = text, fontWeight = FontWeight.Bold, fontSize = 15.sp)
    }
}

@Composable
fun PicooGhostButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    primary: Boolean = false,
) {
    if (primary) {
        PicooPrimaryButton(text = text, onClick = onClick, modifier = modifier)
    } else {
        OutlinedButton(
            onClick = onClick,
            modifier = modifier.fillMaxWidth(),
            shape = RoundedCornerShape(16.dp),
            contentPadding = PaddingValues(vertical = 14.dp, horizontal = 16.dp),
            colors = ButtonDefaults.outlinedButtonColors(
                contentColor = PicooColors.Text,
            ),
            border = androidx.compose.foundation.BorderStroke(1.dp, PicooColors.Line),
        ) {
            Text(text = text, fontWeight = FontWeight.Bold, fontSize = 15.sp)
        }
    }
}

@Composable
fun PicooDangerButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    OutlinedButton(
        onClick = onClick,
        modifier = modifier
            .fillMaxWidth()
            .background(Color(0x24FF5C6C), RoundedCornerShape(16.dp)),
        shape = RoundedCornerShape(16.dp),
        contentPadding = PaddingValues(vertical = 14.dp, horizontal = 16.dp),
        colors = ButtonDefaults.outlinedButtonColors(
            contentColor = PicooColors.DangerText,
        ),
        border = androidx.compose.foundation.BorderStroke(1.dp, Color(0x47FF5C6C)),
    ) {
        Text(text = text, fontWeight = FontWeight.Bold, fontSize = 15.sp)
    }
}

@Composable
fun PicooPill(
    text: String,
    modifier: Modifier = Modifier,
    warn: Boolean = false,
) {
    val bg = if (warn) Color(0x1FF0C14A) else Color(0x1F3ECF8E)
    val fg = if (warn) PicooColors.Warn else PicooColors.Ready
    val border = if (warn) Color(0x47F0C14A) else Color(0x403ECF8E)
    Text(
        text = text,
        modifier = modifier
            .background(bg, RoundedCornerShape(999.dp))
            .border(1.dp, border, RoundedCornerShape(999.dp))
            .padding(horizontal = 10.dp, vertical = 6.dp),
        color = fg,
        fontSize = 12.sp,
        fontWeight = FontWeight.SemiBold,
    )
}

@Composable
fun ReadinessBadge(label: String, paired: Boolean, offline: Boolean = false) {
    val fg = when {
        offline -> PicooColors.Muted
        paired -> PicooColors.Accent2
        else -> PicooColors.Ready
    }
    val bg = when {
        offline -> Color(0x1A656D7D)
        paired -> Color(0x1FFFB347)
        else -> Color(0x1F3ECF8E)
    }
    val border = when {
        offline -> Color(0x38656D7D)
        paired -> Color(0x47FFB347)
        else -> Color(0x383ECF8E)
    }
    Text(
        text = label,
        modifier = Modifier
            .background(bg, RoundedCornerShape(10.dp))
            .border(1.dp, border, RoundedCornerShape(10.dp))
            .padding(horizontal = 8.dp, vertical = 6.dp),
        color = fg,
        fontSize = 11.sp,
        fontWeight = FontWeight.Bold,
        letterSpacing = 0.5.sp,
    )
}
