package com.picoo.camera.ui

import androidx.compose.ui.graphics.Color

/**
 * 取景器曝光补偿的画面反馈（AC-M-LIVE-04）。
 * 不改 ISP，只叠一层明暗滤镜，档位与原型 `☀️ 自动 / 提亮 / 压暗` 对齐。
 */
object ExposurePreview {
    fun overlayColor(ev: Int): Color = when {
        ev > 0 -> Color.White
        ev < 0 -> Color.Black
        else -> Color.Transparent
    }

    fun overlayAlpha(ev: Int): Float = when (ev.coerceIn(-2, 2)) {
        2 -> 0.22f
        1 -> 0.12f
        -1 -> 0.14f
        -2 -> 0.26f
        else -> 0f
    }
}
