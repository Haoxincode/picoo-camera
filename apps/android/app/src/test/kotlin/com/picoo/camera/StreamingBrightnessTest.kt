package com.picoo.camera

import org.junit.Assert.assertTrue
import org.junit.Test

class StreamingBrightnessTest {
    @Test
    fun dimmedLevelIsLowButVisible() {
        assertTrue(StreamingBrightness.DIMMED > 0.05f)
        assertTrue(StreamingBrightness.DIMMED < 0.35f)
    }
}
