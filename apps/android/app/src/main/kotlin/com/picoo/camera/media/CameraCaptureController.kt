package com.picoo.camera.media

import android.util.Size
import android.view.Surface

/**
 * Camera2 + MediaCodec capture configuration (REQ-PICOO-MEDIA-001).
 */
enum class LensFacing {
    Back,
    Front,
}

data class CaptureProfile(
    val resolution: Size = Size(1280, 720),
    val targetFps: Int = 30,
    val lensFacing: LensFacing = LensFacing.Back,
)

enum class CaptureState {
    Idle,
    Previewing,
    Streaming,
    Error,
}

interface CameraCaptureController {
    val state: CaptureState
    val profile: CaptureProfile
    val streamEpoch: Int
    /** Current AE exposure compensation index (device units). */
    val exposureCompensation: Int
    /** Inclusive range supported by the active camera; empty if unknown. */
    val exposureCompensationRange: IntRange

    fun bindPreviewSurface(surface: Surface)
    fun unbindPreviewSurface()
    fun startPreview()
    fun stopPreview()
    /** Apply an epoch allocated by Rust before the next encoder discontinuity. */
    fun prepareStreamEpoch(epoch: Int)
    fun switchCamera()
    /** Switch to a specific lens (desktop CameraCommand SWITCH_FRONT/BACK). */
    fun setLensFacing(facing: LensFacing)
    fun setResolution(width: Int, height: Int)
    fun setTargetBitrateBps(bitrateBps: Int)
    fun requestKeyFrame()
    /** Clamp and apply AE exposure compensation (PUC-005). */
    fun setExposureCompensation(index: Int)
}
