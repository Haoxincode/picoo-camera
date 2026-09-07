package com.picoo.camera.media

import android.hardware.camera2.CameraManager
import android.util.Size

/** Complete requests that all three preparation boundaries admit (MEDIA-056).
 * Call off the UI thread; this queries platform services without opening a camera/codec.
 * A camera service failure is an error, while a valid empty intersection remains empty.
 */
internal object SourcePreparation {
    fun candidates(
        manager: CameraManager,
        facing: LensFacing,
        displayRotationDegrees: Int,
        receiverCandidates: List<VideoSourceFormat>,
        bitrateForFormat: (VideoSourceFormat) -> Int,
    ): Result<List<VideoSourceFormat>> = runCatching {
        val cameraId = CameraCapturePreparation.findCameraId(manager, facing)
            ?: error("Requested camera lens is unavailable")
        val characteristics = manager.getCameraCharacteristics(cameraId)
        receiverCandidates.distinct().filter { source ->
            val size = Size(source.resolution.width, source.resolution.height)
            val bitrate = bitrateForFormat(source)
            require(bitrate > 0) { "Invalid preparation bitrate" }
            runCatching {
                CameraCapturePreparation.chooseCaptureSize(
                    characteristics, size, source.framesPerSecond, facing, displayRotationDegrees,
                )
            }.isSuccess && NativeVideoEncoder.supports(
                NativeEncoderFormat(source.codec, size, source.framesPerSecond, bitrate),
            )
        }
    }
}
