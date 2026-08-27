package com.picoo.camera.media

import android.util.Size

/**
 * Camera2 + MediaCodec capture configuration (REQ-PICOO-MEDIA-001).
 *
 * Full Camera2 InputSurface → H.264 pipeline lands in the next iteration.
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

    fun startPreview()
    fun stopPreview()
    fun switchCamera()
}

/** Placeholder until Camera2 session wiring is implemented. */
class CameraCaptureControllerStub(
    override val profile: CaptureProfile = CaptureProfile(),
) : CameraCaptureController {
    override val state: CaptureState = CaptureState.Idle

    override fun startPreview() {
        // Camera2 capture session + MediaCodec InputSurface (Step 3b)
    }

    override fun stopPreview() = Unit

    override fun switchCamera() = Unit
}
