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
    fun clampsAdaptiveBitrate() {
        assertEquals(500_000, MediaBitrate.clampAdaptive(1))
        assertEquals(12_000_000, MediaBitrate.clampAdaptive(99_000_000))
        assertEquals(4_000_000, MediaBitrate.clampAdaptive(4_000_000))
    }
}
