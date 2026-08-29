package com.picoo.camera.media

/**
 * Clamp AE exposure compensation indices (PUC-005 / REQ-PICOO-MEDIA-008).
 */
object ExposureCompensation {
    fun clamp(index: Int, range: IntRange): Int {
        if (range.isEmpty()) return index
        return index.coerceIn(range.first, range.last)
    }
}
