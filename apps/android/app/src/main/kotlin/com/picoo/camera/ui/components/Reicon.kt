package com.picoo.camera.ui.components

import androidx.annotation.DrawableRes
import androidx.compose.material3.Icon
import androidx.compose.material3.LocalContentColor
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.painterResource
import com.picoo.camera.R

/**
 * Picoo Camera 使用的 Reicon 语义子集。
 *
 * 名称与 `assets/icons/reicon` 的跨平台 SVG 事实源保持一致；Android 仅保存对应的
 * Vector Drawable 转换结果，不依赖完整图标库。
 */
enum class Reicon(@DrawableRes internal val drawableRes: Int) {
    CameraRotate(R.drawable.reicon_camera_rotate),
    ChevronLeft(R.drawable.reicon_chevron_left),
    Clock(R.drawable.reicon_clock),
    Flame(R.drawable.reicon_flame),
    FlipHorizontal(R.drawable.reicon_flip_horizontal),
    Lock(R.drawable.reicon_lock),
    MoreHorizontal(R.drawable.reicon_more_horizontal),
    Refresh(R.drawable.reicon_refresh),
    Settings(R.drawable.reicon_settings),
    Sun(R.drawable.reicon_sun),
    Unlock(R.drawable.reicon_unlock),
    Wifi(R.drawable.reicon_wifi),
    Xmark(R.drawable.reicon_xmark),
}

@Composable
fun ReiconIcon(
    icon: Reicon,
    contentDescription: String?,
    modifier: Modifier = Modifier,
    tint: Color = LocalContentColor.current,
) {
    Icon(
        painter = painterResource(icon.drawableRes),
        contentDescription = contentDescription,
        modifier = modifier,
        tint = tint,
    )
}
