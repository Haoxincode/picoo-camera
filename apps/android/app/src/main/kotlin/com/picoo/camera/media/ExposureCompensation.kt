package com.picoo.camera.media

/**
 * Clamp AE exposure compensation indices (PUC-005 / REQ-PICOO-MEDIA-008).
 */
object ExposureCompensation {
    private val controlPresets = intArrayOf(0, 1, 2, -1, -2)

    fun clamp(index: Int, range: IntRange): Int {
        if (range.isEmpty()) return index
        return index.coerceIn(range.first, range.last)
    }

    /**
     * Return the next exposure value offered by the connected-preview control.
     * Unsupported presets are skipped so the same interaction works across Camera2 devices.
     */
    fun nextControlPreset(current: Int, range: IntRange): Int {
        if (range.isEmpty()) return current
        val availablePresets = controlPresets.filter { it in range }
        if (availablePresets.isEmpty()) return clamp(0, range)
        val currentIndex = availablePresets.indexOf(current)
        return availablePresets[(currentIndex + 1).mod(availablePresets.size)]
    }
}
