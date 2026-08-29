package com.picoo.camera.media

/**
 * Negotiated stream resolutions for Sender (REQ-PICOO-UI-0001 AC-M-LIVE-01 / PUC-005).
 */
enum class StreamResolution(val label: String, val width: Int, val height: Int) {
    P1080("1080p", 1920, 1080),
    P720("720p", 1280, 720),
    P480("480p", 854, 480),
    ;

    fun displayPill(): String = "${label.removeSuffix("p").uppercase()}P · 30"

    companion object {
        private val cycle = listOf(P1080, P720, P480)

        fun fromLabel(label: String): StreamResolution =
            entries.firstOrNull { it.label.equals(label, ignoreCase = true) } ?: P720

        fun fromHeight(height: Int): StreamResolution = when {
            height >= 1080 -> P1080
            height >= 720 -> P720
            else -> P480
        }

        /** Tap-to-cycle: 1080 → 720 → 480 → 1080; thermal mode only 720 ↔ 480. */
        fun next(current: StreamResolution, thermalForced720: Boolean): StreamResolution {
            if (thermalForced720) {
                return if (current == P480) P720 else P480
            }
            val idx = cycle.indexOf(current).coerceAtLeast(0)
            return cycle[(idx + 1) % cycle.size]
        }
    }
}
