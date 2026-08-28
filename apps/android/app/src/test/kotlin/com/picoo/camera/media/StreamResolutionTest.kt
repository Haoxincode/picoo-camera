package com.picoo.camera.media

import org.junit.Assert.assertEquals
import org.junit.Test

class StreamResolutionTest {
    @Test
    fun cyclesThreeTiersWhenNotThermal() {
        assertEquals(StreamResolution.P720, StreamResolution.next(StreamResolution.P1080, false))
        assertEquals(StreamResolution.P480, StreamResolution.next(StreamResolution.P720, false))
        assertEquals(StreamResolution.P1080, StreamResolution.next(StreamResolution.P480, false))
    }

    @Test
    fun thermalCyclesOnly720And480() {
        assertEquals(StreamResolution.P480, StreamResolution.next(StreamResolution.P720, true))
        assertEquals(StreamResolution.P720, StreamResolution.next(StreamResolution.P480, true))
        assertEquals(StreamResolution.P480, StreamResolution.next(StreamResolution.P1080, true))
    }

    @Test
    fun fromHeightMapsTiers() {
        assertEquals(StreamResolution.P1080, StreamResolution.fromHeight(1080))
        assertEquals(StreamResolution.P720, StreamResolution.fromHeight(720))
        assertEquals(StreamResolution.P480, StreamResolution.fromHeight(480))
    }
}
