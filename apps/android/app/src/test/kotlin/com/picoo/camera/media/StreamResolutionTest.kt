package com.picoo.camera.media

import org.junit.Assert.assertNull
import org.junit.Assert.assertEquals
import org.junit.Test

class StreamResolutionTest {
    @Test
    fun sourceHeightMustMatchExactly() {
        assertEquals(StreamResolution.P1080, StreamResolution.fromHeight(1080))
        assertEquals(StreamResolution.P720, StreamResolution.fromHeight(720))
        for (height in listOf(0, 480, 719, 721, 1079, 1081, 2160)) {
            assertNull(StreamResolution.fromHeight(height))
        }
    }
}
