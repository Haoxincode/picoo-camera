package com.picoo.camera.media

import android.hardware.camera2.CameraDevice
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicReference

/** Generation tokens and locks that keep Camera2 and MediaCodec rebuilds on the same generation. */
internal class CaptureLifecycle {
    val cameraGeneration = AtomicLong(0)
    val codecGeneration = AtomicLong(0)
    val captureSessionGeneration = AtomicLong(0)
    val reopenAfterCameraGeneration = AtomicLong(-1)
    val cameraLifecycleLock = Any()
    val codecLifecycleLock = Any()
    val outputSurfaceLock = Any()

    private val _state = AtomicReference(CaptureState.Idle)

    val state: CaptureState
        get() = _state.get()

    fun setState(value: CaptureState) {
        _state.set(value)
    }

    fun compareAndSetState(expect: CaptureState, update: CaptureState): Boolean =
        _state.compareAndSet(expect, update)

    fun isCurrentCodecTransition(
        generation: Long,
        camera: CameraDevice,
        cameraGenerationSnapshot: Long,
        currentCamera: CameraDevice?,
    ): Boolean = generation == codecGeneration.get() &&
        cameraGenerationSnapshot == cameraGeneration.get() &&
        camera === currentCamera

    fun isCurrentCaptureSession(
        sessionGeneration: Long,
        codecGenerationSnapshot: Long,
        camera: CameraDevice,
        currentCamera: CameraDevice?,
    ): Boolean = sessionGeneration == captureSessionGeneration.get() &&
        codecGenerationSnapshot == codecGeneration.get() &&
        camera === currentCamera
}
