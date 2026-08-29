package com.picoo.camera.media

/**
 * Default H.264 bitrate ladders for negotiated 720p/1080p/480p (REQ-PICOO-MEDIA-001 / MEDIA-007).
 */
object MediaBitrate {
    data class Ladder(val minBps: Int, val initialBps: Int, val maxBps: Int)

    fun forResolution(width: Int, height: Int): Int = when {
        height >= 1080 || width.toLong() * height >= 1920L * 1080L -> 6_000_000
        height >= 720 -> 3_000_000
        else -> 1_800_000
    }

    fun ladderForHeight(height: Int): Ladder = when {
        height >= 1080 -> Ladder(minBps = 3_000_000, initialBps = 6_000_000, maxBps = 10_000_000)
        height >= 720 -> Ladder(minBps = 1_500_000, initialBps = 3_000_000, maxBps = 5_000_000)
        else -> Ladder(minBps = 900_000, initialBps = 1_800_000, maxBps = 2_500_000)
    }

    fun clampAdaptive(bitrateBps: Int, height: Int = 1080): Int {
        val ladder = ladderForHeight(height)
        return bitrateBps.coerceIn(ladder.minBps, ladder.maxBps)
    }
}
