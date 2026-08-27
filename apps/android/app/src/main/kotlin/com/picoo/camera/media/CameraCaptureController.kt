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

    fun bindPreviewSurface(surface: Surface)
    fun unbindPreviewSurface()
    fun startPreview()
    fun stopPreview()
    fun switchCamera()
}
