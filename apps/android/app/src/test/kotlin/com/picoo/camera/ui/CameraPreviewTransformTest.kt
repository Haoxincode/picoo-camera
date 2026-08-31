package com.picoo.camera.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** REQ-PICOO-MEDIA-001: the local Camera2 preview preserves aspect ratio. */
class CameraPreviewTransformTest {
    @Test
    fun portrait16By9NeedsNoNonUniformScale() {
        val transform = calculatePreviewTransform(
            viewWidth = 1080,
            viewHeight = 1920,
            bufferWidth = 1920,
            bufferHeight = 1080,
            sensorOrientationDegrees = 90,
            displayRotationDegrees = 0,
            frontFacing = false,
        )

        assertEquals(1f, transform.scaleX, 0.0001f)
        assertEquals(1f, transform.scaleY, 0.0001f)
        assertEquals(0f, transform.rotationDegrees, 0.0001f)
    }

    @Test
    fun outputViewportCenterCropsCameraWithoutStretching() {
        val transform = calculatePreviewTransform(
            viewWidth = 1080,
            viewHeight = 2400,
            bufferWidth = 1920,
            bufferHeight = 1080,
            sensorOrientationDegrees = 90,
            displayRotationDegrees = 0,
            frontFacing = true,
        )

        assertTrue(transform.scaleX > 1f || transform.scaleY > 1f)
    }

    @Test
    fun displayRotationIsCompensated() {
        val transform = calculatePreviewTransform(
            viewWidth = 1920,
            viewHeight = 1080,
            bufferWidth = 1920,
            bufferHeight = 1080,
            sensorOrientationDegrees = 90,
            displayRotationDegrees = 90,
            frontFacing = false,
        )

        assertEquals(-90f, transform.rotationDegrees, 0.0001f)
    }

    @Test
    fun preApi33FrontCameraCompensatesPlatformAutoMirror() {
        assertEquals(
            false,
            needsTextureViewMirror(
                mirrorLocal = true,
                frontFacing = true,
                platformMirrorCanBeDisabled = false,
            ),
        )
        assertEquals(
            true,
            needsTextureViewMirror(
                mirrorLocal = false,
                frontFacing = true,
                platformMirrorCanBeDisabled = false,
            ),
        )
    }

    @Test
    fun api33UsesExplicitUiMirrorAfterDisablingPlatformMirror() {
        assertEquals(
            true,
            needsTextureViewMirror(
                mirrorLocal = true,
                frontFacing = true,
                platformMirrorCanBeDisabled = true,
            ),
        )
        assertEquals(
            false,
            needsTextureViewMirror(
                mirrorLocal = false,
                frontFacing = true,
                platformMirrorCanBeDisabled = true,
            ),
        )
    }

    @Test
    fun allCardinalSensorAndDisplayRotationsRemainFinite() {
        for (sensor in listOf(0, 90, 180, 270)) {
            for (display in listOf(0, 90, 180, 270)) {
                for (frontFacing in listOf(false, true)) {
                    val transform = calculatePreviewTransform(
                        viewWidth = 1080,
                        viewHeight = 2400,
                        bufferWidth = 1440,
                        bufferHeight = 1080,
                        sensorOrientationDegrees = sensor,
                        displayRotationDegrees = display,
                        frontFacing = frontFacing,
                    )
                    assertTrue(transform.scaleX.isFinite() && transform.scaleX > 0f)
                    assertTrue(transform.scaleY.isFinite() && transform.scaleY > 0f)
                    assertEquals(-display.toFloat(), transform.rotationDegrees, 0.0001f)
                }
            }
        }
    }

    @Test
    fun fourByThreeOemSourceCenterCropsInsideOutputViewport() {
        val transform = calculatePreviewTransform(
            viewWidth = 1080,
            viewHeight = 2400,
            bufferWidth = 1440,
            bufferHeight = 1080,
            sensorOrientationDegrees = 90,
            displayRotationDegrees = 0,
            frontFacing = false,
        )

        assertTrue(transform.scaleX > 1f || transform.scaleY > 1f)
    }
}
