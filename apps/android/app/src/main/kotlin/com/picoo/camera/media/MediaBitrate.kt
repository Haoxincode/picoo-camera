package com.picoo.camera.media

/**
 * Default H.264 bitrate ladders for negotiated 720p/1080p (REQ-PICOO-MEDIA-001 / MEDIA-007).
 */
object MediaBitrate {
    fun forResolution(width: Int, height: Int): Int {
        val pixels = width.toLong() * height.toLong()
        return if (pixels >= 1920L * 1080L) 6_000_000 else 3_000_000
    }

    fun clampAdaptive(bitrateBps: Int): Int = bitrateBps.coerceIn(500_000, 12_000_000)
}
