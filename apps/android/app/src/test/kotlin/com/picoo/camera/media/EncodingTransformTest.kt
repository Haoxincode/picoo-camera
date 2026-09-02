package com.picoo.camera.media

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class EncodingTransformTest {
    @Test
    fun landscapeBufferWithNoRotationUsesCompleteFrame() {
        val matrix = transform(
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
        val matrix = transform(
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
        val matrix = transform(
            cameraBufferWidth = 1280,
            cameraBufferHeight = 720,
            outputWidth = 1280,
            outputHeight = 720,
            clockwiseRotationDegrees = 180,
        )

        assertPoint(matrix, 0f, 0f, 1f, 1f)
        assertPoint(matrix, 1f, 1f, 0f, 0f)
    }

    @Test
    fun producerAxisSwapCropsFourByThreeSourceWithoutStretching() {
        val matrix = EncodingTransform.outputToSurfaceTextureMatrix(
            cameraBufferWidth = 1920,
            cameraBufferHeight = 1440,
            outputWidth = 1280,
            outputHeight = 720,
            clockwiseRotationDegrees = 90,
            producerTextureMatrix = PRODUCER_AXIS_SWAP,
        )

        // This producer matrix already swaps the texture axes. After the
        // requested rotation cancels that swap, the 16:9 crop must retain the
        // full 1920px width and a centered 1080px band, not a 608px band.
        assertPoint(matrix, 0f, 0f, 0f, 0.875f)
        assertPoint(matrix, 1f, 1f, 1f, 0.125f)
        assertEquals(16f / 9f, sampledAspect(matrix, 1920, 1440), 0.0001f)
    }

    @Test
    fun producerAxisSwapKeepsLandscape480SourceUndistorted() {
        val matrix = EncodingTransform.outputToSurfaceTextureMatrix(
            cameraBufferWidth = 1280,
            cameraBufferHeight = 960,
            outputWidth = 854,
            outputHeight = 480,
            clockwiseRotationDegrees = 0,
            producerTextureMatrix = PRODUCER_AXIS_SWAP,
        )

        assertEquals(854f / 480f, sampledAspect(matrix, 1280, 960), 0.0001f)
    }

    @Test
    fun producerAxisSwapKeepsSquare1080SourceUndistorted() {
        val matrix = EncodingTransform.outputToSurfaceTextureMatrix(
            cameraBufferWidth = 2448,
            cameraBufferHeight = 2448,
            outputWidth = 1920,
            outputHeight = 1080,
            clockwiseRotationDegrees = 90,
            producerTextureMatrix = PRODUCER_AXIS_SWAP,
        )

        assertEquals(16f / 9f, sampledAspect(matrix, 2448, 2448), 0.0001f)
    }

    private fun transform(
        cameraBufferWidth: Int,
        cameraBufferHeight: Int,
        outputWidth: Int,
        outputHeight: Int,
        clockwiseRotationDegrees: Int,
    ): FloatArray = EncodingTransform.outputToSurfaceTextureMatrix(
        cameraBufferWidth = cameraBufferWidth,
        cameraBufferHeight = cameraBufferHeight,
        outputWidth = outputWidth,
        outputHeight = outputHeight,
        clockwiseRotationDegrees = clockwiseRotationDegrees,
        producerTextureMatrix = IDENTITY_MATRIX,
    )

    private fun sampledAspect(matrix: FloatArray, width: Int, height: Int): Float {
        val origin = EncodingTransform.mapPoint(matrix, 0f, 0f)
        val xEnd = EncodingTransform.mapPoint(matrix, 1f, 0f)
        val yEnd = EncodingTransform.mapPoint(matrix, 0f, 1f)
        val sampledWidth = kotlin.math.hypot(
            (xEnd.first - origin.first) * width,
            (xEnd.second - origin.second) * height,
        )
        val sampledHeight = kotlin.math.hypot(
            (yEnd.first - origin.first) * width,
            (yEnd.second - origin.second) * height,
        )
        return sampledWidth / sampledHeight
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

    private companion object {
        val IDENTITY_MATRIX = floatArrayOf(
            1f, 0f, 0f, 0f,
            0f, 1f, 0f, 0f,
            0f, 0f, 1f, 0f,
            0f, 0f, 0f, 1f,
        )
        val PRODUCER_AXIS_SWAP = floatArrayOf(
            0f, -1f, 0f, 0f,
            -1f, 0f, 0f, 0f,
            0f, 0f, 1f, 0f,
            1f, 1f, 0f, 1f,
        )
    }
}
