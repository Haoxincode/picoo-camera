package com.picoo.camera.ui

import com.picoo.camera.media.StreamResolution

/** 设置页默认画质 sheet 文案，对齐原型 `m-sheet-res`（AC-M-SET-02）。 */
data class ResolutionSheetOption(
    val resolution: StreamResolution,
    val title: String,
    val subtitle: String,
)

object ResolutionSheetOptions {
    val all: List<ResolutionSheetOption> = listOf(
        ResolutionSheetOption(
            StreamResolution.P1080,
            "1080p30 (Full HD)",
            "推荐带宽 ≥ 5.2 Mbps",
        ),
        ResolutionSheetOption(
            StreamResolution.P720,
            "720p30 (Standard HD)",
            "推荐带宽 ≥ 3.4 Mbps · 功耗平衡",
        ),
        ResolutionSheetOption(
            StreamResolution.P480,
            "480p30 (Low Bandwidth)",
            "推荐带宽 ≥ 1.8 Mbps · 弱网保底",
        ),
    )
}
