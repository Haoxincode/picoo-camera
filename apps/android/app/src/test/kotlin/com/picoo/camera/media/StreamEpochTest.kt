package com.picoo.camera.media

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** REQ-PICOO-MEDIA-003: switchCamera / setResolution bump stream_epoch. */
class StreamEpochTest {
    @Test
    fun initialEpochIsOne() {
        assertEquals(1, StreamEpoch.INITIAL)
    }

    @Test
    fun bumpIncrementsMonotonically() {
        assertEquals(2, StreamEpoch.bump(StreamEpoch.INITIAL))
        assertEquals(3, StreamEpoch.bump(2))
        assertEquals(4, StreamEpoch.bump(3))
    }

    @Test
    fun switchCameraAlwaysBumps() {
        // Lens flip always changes capture path → epoch++ then IDR (Camera2MediaEncoder).
        val before = 5
        assertEquals(6, StreamEpoch.bump(before))
    }

    @Test
    fun resolutionChangeBumpsOnlyWhenSizeDiffers() {
        assertTrue(StreamEpoch.shouldBumpForResolution(1280, 720, 1920, 1080))
        assertTrue(StreamEpoch.shouldBumpForResolution(1920, 1080, 1280, 720))
        assertFalse(StreamEpoch.shouldBumpForResolution(1280, 720, 1280, 720))
    }

    @Test
    fun simulatedSwitchThenResolutionSequence() {
        var epoch = StreamEpoch.INITIAL
        epoch = StreamEpoch.bump(epoch) // switchCamera
        assertEquals(2, epoch)
        assertTrue(StreamEpoch.shouldBumpForResolution(1280, 720, 1920, 1080))
        epoch = StreamEpoch.bump(epoch) // setResolution
        assertEquals(3, epoch)
    }
}
