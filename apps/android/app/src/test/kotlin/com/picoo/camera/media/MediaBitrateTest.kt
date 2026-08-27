package com.picoo.camera.media

import org.junit.Assert.assertEquals
import org.junit.Test

class MediaBitrateTest {
    @Test
    fun usesThreeMbpsFor720p() {
        assertEquals(3_000_000, MediaBitrate.forResolution(1280, 720))
    }

    @Test
    fun usesSixMbpsFor1080p() {
        assertEquals(6_000_000, MediaBitrate.forResolution(1920, 1080))
    }

    @Test
    fun laddersMatchAbrBounds() {
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
