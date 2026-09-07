package com.picoo.camera.media

import android.graphics.SurfaceTexture
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraManager
import android.util.Range
import android.util.Size

/** One official Camera2 admission rule shared by preparation and capture (MEDIA-050/056). */
internal object CameraCapturePreparation {
    fun findCameraId(manager: CameraManager, facing: LensFacing): String? {
        val target = when (facing) {
            LensFacing.Back -> CameraCharacteristics.LENS_FACING_BACK
            LensFacing.Front -> CameraCharacteristics.LENS_FACING_FRONT
        }
        return manager.cameraIdList.firstOrNull { id ->
            manager.getCameraCharacteristics(id)
                .get(CameraCharacteristics.LENS_FACING) == target
        }
    }

    fun chooseCaptureSize(
        characteristics: CameraCharacteristics,
        target: Size,
        fps: Int,
        facing: LensFacing,
        displayRotationDegrees: Int,
    ): Size {
        val map = characteristics
            .get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP)
            ?: error("Camera stream configuration is unavailable")
        require(fps == 30 || fps == 60) { "Unsupported source frame rate" }
        val fixedRate = Range(fps, fps)
        check(characteristics.get(CameraCharacteristics.CONTROL_AE_AVAILABLE_TARGET_FPS_RANGES)
            .orEmpty().contains(fixedRate)) { "Camera does not support the requested fixed frame rate" }
        val maxFrameDurationNanos = 1_000_000_000L / fps
        val outputSizes = map.getOutputSizes(SurfaceTexture::class.java).orEmpty()
        val frameRateCapable = outputSizes.filter { size ->
            val duration = map.getOutputMinFrameDuration(SurfaceTexture::class.java, size)
            duration > 0L && duration <= maxFrameDurationNanos
        }
        val sensorOrientation =
            characteristics.get(CameraCharacteristics.SENSOR_ORIENTATION)
                ?: error("Camera sensor orientation is unavailable")
        val portraitCrop = StreamOrientation.relativeRotationDegrees(
            sensorOrientationDegrees = sensorOrientation,
            displayRotationDegrees = displayRotationDegrees,
            frontFacing = facing == LensFacing.Front,
        ) % 180 != 0
        val choices = frameRateCapable
            .map { CaptureSizeSelector.Dim(it.width, it.height) }
        val selected = CaptureSizeSelector.select(
            choices,
            CaptureSizeSelector.Dim(target.width, target.height),
            portraitCrop = portraitCrop,
        )
        return Size(selected.width, selected.height)
    }
}
