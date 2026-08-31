package com.picoo.camera.media

import kotlin.math.abs

/**
 * Pure texture-coordinate transform for the Android encoding compositor
 * (REQ-PICOO-MEDIA-013).
 *
 * The returned column-major matrix maps normalized output coordinates into the
 * Camera2 OES texture before SurfaceTexture's producer matrix is applied.
 */
object EncodingTransform {
    fun outputToCameraTextureMatrix(
        cameraBufferWidth: Int,
        cameraBufferHeight: Int,
        outputWidth: Int,
        outputHeight: Int,
        clockwiseRotationDegrees: Int,
    ): FloatArray {
        require(cameraBufferWidth > 0 && cameraBufferHeight > 0)
        require(outputWidth > 0 && outputHeight > 0)

        val rotation = normalizeCardinalRotation(clockwiseRotationDegrees)
        val swapsAxes = rotation == 90 || rotation == 270
        val uprightWidth = if (swapsAxes) cameraBufferHeight else cameraBufferWidth
        val uprightHeight = if (swapsAxes) cameraBufferWidth else cameraBufferHeight
        val uprightAspect = uprightWidth.toFloat() / uprightHeight.toFloat()
        val outputAspect = outputWidth.toFloat() / outputHeight.toFloat()
        val sampleScaleX: Float
        val sampleScaleY: Float
        if (uprightAspect > outputAspect) {
            sampleScaleX = outputAspect / uprightAspect
            sampleScaleY = 1f
        } else {
            sampleScaleX = 1f
            sampleScaleY = uprightAspect / outputAspect
        }

        val crop = identityMatrix().apply {
            this[0] = sampleScaleX
            this[5] = sampleScaleY
            this[12] = (1f - sampleScaleX) / 2f
            this[13] = (1f - sampleScaleY) / 2f
        }
        return multiply(rotationInverseMatrix(rotation), crop)
    }

    fun multiply(left: FloatArray, right: FloatArray): FloatArray {
        require(left.size == MATRIX_SIZE && right.size == MATRIX_SIZE)
        val result = FloatArray(MATRIX_SIZE)
        for (column in 0 until 4) {
            for (row in 0 until 4) {
                var value = 0f
                for (index in 0 until 4) {
                    value += left[index * 4 + row] * right[column * 4 + index]
                }
                result[column * 4 + row] = value
            }
        }
        return result
    }

    internal fun mapPoint(matrix: FloatArray, x: Float, y: Float): Pair<Float, Float> {
        require(matrix.size == MATRIX_SIZE)
        return Pair(
            matrix[0] * x + matrix[4] * y + matrix[12],
            matrix[1] * x + matrix[5] * y + matrix[13],
        )
    }

    internal fun matricesApproximatelyEqual(
        left: FloatArray,
        right: FloatArray,
        tolerance: Float = 0.0001f,
    ): Boolean = left.size == right.size && left.indices.all { index ->
        abs(left[index] - right[index]) <= tolerance
    }

    private fun normalizeCardinalRotation(rotation: Int): Int {
        val normalized = ((rotation % 360) + 360) % 360
        require(normalized % 90 == 0) { "rotation must be 0, 90, 180, or 270" }
        return normalized
    }

    private fun rotationInverseMatrix(rotation: Int): FloatArray = when (rotation) {
        0 -> identityMatrix()
        90 -> floatArrayOf(
            0f, -1f, 0f, 0f,
            1f, 0f, 0f, 0f,
            0f, 0f, 1f, 0f,
            0f, 1f, 0f, 1f,
        )
        180 -> floatArrayOf(
            -1f, 0f, 0f, 0f,
            0f, -1f, 0f, 0f,
            0f, 0f, 1f, 0f,
            1f, 1f, 0f, 1f,
        )
        270 -> floatArrayOf(
            0f, 1f, 0f, 0f,
            -1f, 0f, 0f, 0f,
            0f, 0f, 1f, 0f,
            1f, 0f, 0f, 1f,
        )
        else -> error("unreachable")
    }

    private fun identityMatrix(): FloatArray = floatArrayOf(
        1f, 0f, 0f, 0f,
        0f, 1f, 0f, 0f,
        0f, 0f, 1f, 0f,
        0f, 0f, 0f, 1f,
    )

    private const val MATRIX_SIZE = 16
}
