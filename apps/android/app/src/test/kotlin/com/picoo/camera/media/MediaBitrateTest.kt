package com.picoo.camera.media

import org.junit.Assert.assertEquals
import org.junit.Test

class MediaBitrateTest {
    @Test
    fun usesOnePointEightMbpsFor480p() {
        assertEquals(1_800_000, MediaBitrate.forResolution(854, 480))
    }

    fun usesThreeMbpsFor720p() {
        assertEquals(3_000_000, MediaBitrate.forResolution(1280, 720))
    }

    @Test
    fun usesSixMbpsFor1080p() {
        assertEquals(6_000_000, MediaBitrate.forResolution(1920, 1080))
    }

    @Test
    fun laddersMatchAbrBounds() {
        val ladder480 = MediaBitrate.ladderForHeight(480)
        assertEquals(900_000, ladder480.minBps)
        assertEquals(1_800_000, ladder480.initialBps)
        assertEquals(2_500_000, ladder480.maxBps)
        val ladder720 = MediaBitrate.ladderForHeight(720)
        assertEquals(1_500_000, ladder720.minBps)
        assertEquals(5_000_000, ladder720.maxBps)
        val ladder1080 = MediaBitrate.ladderForHeight(1080)
        assertEquals(3_000_000, ladder1080.minBps)
        assertEquals(10_000_000, ladder1080.maxBps)
    }

    @Test
    fun clampsAdaptiveBitrateToActiveLadder() {
        assertEquals(1_500_000, MediaBitrate.clampAdaptive(1, 720))
        assertEquals(5_000_000, MediaBitrate.clampAdaptive(99_000_000, 720))
        assertEquals(3_000_000, MediaBitrate.clampAdaptive(1, 1080))
        assertEquals(10_000_000, MediaBitrate.clampAdaptive(99_000_000, 1080))
    }
}
