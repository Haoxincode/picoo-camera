package com.picoo.camera.media

/**
 * Negotiated stream resolutions for Sender (REQ-PICOO-UI-0001 AC-M-LIVE-01 / PUC-005).
 */
enum class StreamResolution(val label: String, val width: Int, val height: Int) {
    P1080("1080p", 1920, 1080),
    P720("720p", 1280, 720),
    ;

    fun displayPill(): String = "${label.removeSuffix("p").uppercase()}P · 30"

    companion object {
        private val cycle = listOf(P1080, P720)

        fun fromLabel(label: String): StreamResolution? =
            entries.firstOrNull { it.label == label }

        /** REQ-PICOO-MEDIA-028: unknown native/config values are not another format. */
        fun fromHeight(height: Int): StreamResolution? =
            entries.firstOrNull { it.height == height }

        /** A user action cycles formats; thermal state never substitutes another format. */
        fun next(current: StreamResolution): StreamResolution {
            val idx = cycle.indexOf(current).coerceAtLeast(0)
            return cycle[(idx + 1) % cycle.size]
        }
    }
}
