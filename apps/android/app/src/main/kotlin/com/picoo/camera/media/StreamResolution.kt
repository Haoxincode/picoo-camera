package com.picoo.camera.media

/** Formal visible source dimensions; codec and frame rate belong to VideoSourceFormat. */
enum class StreamResolution(val label: String, val width: Int, val height: Int) {
    P1080("1080p", 1920, 1080),
    P720("720p", 1280, 720),
    ;

    companion object {
        /** Unknown dimensions must not select another format (MEDIA-028). */
        fun fromHeight(height: Int): StreamResolution? = entries.firstOrNull { it.height == height }
    }
}
