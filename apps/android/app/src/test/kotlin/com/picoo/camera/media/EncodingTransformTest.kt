package com.picoo.camera.media

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class EncodingTransformTest {
    @Test
    fun landscapeBufferWithNoRotationUsesCompleteFrame() {
        val matrix = EncodingTransform.outputToCameraTextureMatrix(
            cameraBufferWidth = 1280,
            cameraBufferHeight = 720,
            outputWidth = 1280,
            outputHeight = 720,
            clockwiseRotationDegrees = 0,
        )

        assertPoint(matrix, 0f, 0f, 0f, 0f)
        assertPoint(matrix, 1f, 1f, 1f, 1f)
    }

    @Test
    fun portraitPhoneSelectsCentralHorizontalBandBeforeEncoding() {
        val matrix = EncodingTransform.outputToCameraTextureMatrix(
            cameraBufferWidth = 1280,
            cameraBufferHeight = 720,
            outputWidth = 1280,
            outputHeight = 720,
            clockwiseRotationDegrees = 90,
        )

        // Upright portrait is 720×1280. A 16:9 output uses its full width and
        // central 405px height, which is the central X band in sensor space.
        assertPoint(matrix, 0f, 0f, 0.34179688f, 1f)
        assertPoint(matrix, 1f, 1f, 0.6582031f, 0f)
        assertPoint(matrix, 0.5f, 0.5f, 0.5f, 0.5f)
    }

    @Test
    fun reverseLandscapeRotatesPixelsWithoutCropping() {
        val matrix = EncodingTransform.outputToCameraTextureMatrix(
            cameraBufferWidth = 1280,
            cameraBufferHeight = 720,
            outputWidth = 1280,
            outputHeight = 720,
            clockwiseRotationDegrees = 180,
        )

        assertPoint(matrix, 0f, 0f, 1f, 1f)
        assertPoint(matrix, 1f, 1f, 0f, 0f)
    }

    private fun assertPoint(
        matrix: FloatArray,
        x: Float,
        y: Float,
        expectedX: Float,
        expectedY: Float,
    ) {
        val actual = EncodingTransform.mapPoint(matrix, x, y)
        assertEquals(expectedX, actual.first, 0.0001f)
        assertEquals(expectedY, actual.second, 0.0001f)
        assertTrue(actual.first in 0f..1f)
        assertTrue(actual.second in 0f..1f)
    }
}
