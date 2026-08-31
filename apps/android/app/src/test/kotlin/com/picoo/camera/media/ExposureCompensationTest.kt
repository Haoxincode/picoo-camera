package com.picoo.camera.media

import org.junit.Assert.assertEquals
import org.junit.Test

class ExposureCompensationTest {
    @Test
    fun clampsWithinRange() {
        assertEquals(2, ExposureCompensation.clamp(2, -2..2))
        assertEquals(2, ExposureCompensation.clamp(9, -2..2))
        assertEquals(-2, ExposureCompensation.clamp(-9, -2..2))
    }

    @Test
    fun passesThroughWhenRangeEmpty() {
        assertEquals(5, ExposureCompensation.clamp(5, IntRange.EMPTY))
    }

    @Test
    fun cyclesThroughConnectedPreviewPresets() {
        assertEquals(1, ExposureCompensation.nextControlPreset(0, -2..2))
        assertEquals(2, ExposureCompensation.nextControlPreset(1, -2..2))
        assertEquals(-1, ExposureCompensation.nextControlPreset(2, -2..2))
        assertEquals(-2, ExposureCompensation.nextControlPreset(-1, -2..2))
        assertEquals(0, ExposureCompensation.nextControlPreset(-2, -2..2))
    }

    @Test
    fun skipsPresetsUnsupportedByCamera() {
        assertEquals(1, ExposureCompensation.nextControlPreset(0, -1..1))
        assertEquals(-1, ExposureCompensation.nextControlPreset(1, -1..1))
        assertEquals(0, ExposureCompensation.nextControlPreset(-1, -1..1))
    }
}
