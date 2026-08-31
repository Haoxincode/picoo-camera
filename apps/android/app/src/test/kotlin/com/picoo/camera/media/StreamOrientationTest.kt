package com.picoo.camera.media

import org.junit.Assert.assertEquals
import org.junit.Test

class StreamOrientationTest {
    @Test
    fun rearCameraUsesRotationRelativeToDisplay() {
        assertEquals(90, StreamOrientation.relativeRotationDegrees(90, 0, false))
        assertEquals(0, StreamOrientation.relativeRotationDegrees(90, 90, false))
        assertEquals(270, StreamOrientation.relativeRotationDegrees(90, 180, false))
        assertEquals(180, StreamOrientation.relativeRotationDegrees(90, 270, false))
    }

    @Test
    fun frontCameraUsesMirroredOrientationConvention() {
        assertEquals(270, StreamOrientation.relativeRotationDegrees(270, 0, true))
        assertEquals(0, StreamOrientation.relativeRotationDegrees(270, 90, true))
        assertEquals(90, StreamOrientation.relativeRotationDegrees(270, 180, true))
        assertEquals(180, StreamOrientation.relativeRotationDegrees(270, 270, true))
    }
}
