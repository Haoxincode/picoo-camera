package com.picoo.camera.media

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class LinkQualityTest {
    @Test
    fun labelsByLossAndRtt() {
        assertEquals("Excellent", LinkQuality.label(0.0, 10.0))
        assertEquals("Good", LinkQuality.label(0.01, 40.0))
        assertEquals("Fair", LinkQuality.label(0.03, 20.0))
        assertEquals("Poor", LinkQuality.label(0.06, 20.0))
        assertEquals("Poor", LinkQuality.label(0.0, 150.0))
    }

    @Test
    fun formatsStreamingLine() {
        val line = LinkQuality.formatLine(25.0, 0.012, 40.0, 3_200_000.0)
        assertTrue(line.contains("Good"))
        assertTrue(line.contains("RTT 25ms"))
        assertTrue(line.contains("rx 3200kbps"))
    }
}
