package com.picoo.camera.media

import android.Manifest
import android.content.pm.PackageManager
import android.graphics.SurfaceTexture
import android.hardware.camera2.CameraCaptureSession
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraDevice
import android.hardware.camera2.CaptureRequest
import android.hardware.camera2.CaptureResult
import android.hardware.camera2.TotalCaptureResult
import android.hardware.camera2.params.OutputConfiguration
import android.hardware.camera2.params.SessionConfiguration
import android.os.Build
import android.util.Range
import android.util.Size
import android.view.Surface
import androidx.core.content.ContextCompat
import java.util.concurrent.Executor

/** Camera2 open / session / preview / exposure (MEDIA-001). */
internal class Camera2DeviceSession(
    private val encoder: Camera2MediaEncoder,
) {
    fun bindPreviewSurface(surfaceTexture: SurfaceTexture) {
        val changed = encoder.previewSurfaceTexture !== surfaceTexture
        if (changed) {
            val bufferSize = encoder.previewTransformInfo.bufferSize
            val nextSurface = runCatching {
                surfaceTexture.setDefaultBufferSize(bufferSize.width, bufferSize.height)
                Surface(surfaceTexture)
            }.onFailure {
                encoder.lastError = "Preview surface bind failed: ${it.message}"
            }.getOrNull() ?: return
            closeCaptureSession()
            synchronized(encoder.lifecycle.outputSurfaceLock) {
                encoder.previewSurface?.release()
                encoder.previewSurfaceTexture = surfaceTexture
                encoder.previewSurface = nextSurface
            }
        }
        if (changed) {
            scheduleCaptureSessionRebuild(surfaceTexture)
        }
    }

    fun unbindPreviewSurface(surfaceTexture: SurfaceTexture) {
        if (encoder.previewSurfaceTexture !== surfaceTexture) return
        closeCaptureSession()
        val removed = synchronized(encoder.lifecycle.outputSurfaceLock) {
            if (encoder.previewSurfaceTexture !== surfaceTexture) {
                false
            } else {
                encoder.previewSurfaceTexture = null
                encoder.previewSurface?.release()
                encoder.previewSurface = null
                true
            }
        }
        if (!removed) return
        // Keep H.264 encode alive when the Compose TextureView is torn down
        // (tab switch / config change); rebuild a codec-only Camera2 session.
        scheduleCaptureSessionRebuild(expectedPreviewSurfaceTexture = null)
    }

    /**
     * Reconcile preview-surface changes on the Camera2 callback thread.
     *
     * CameraDevice callbacks can clear [Camera2MediaEncoder.cameraDevice] while Compose binds or
     * destroys its SurfaceTexture on the main thread. Posting the reconciliation
     * avoids a check-then-force-unwrap race and the identity check prevents a
     * stale bind/unbind callback from rebuilding the current session.
     */
    private fun scheduleCaptureSessionRebuild(
        expectedPreviewSurfaceTexture: SurfaceTexture?,
    ) {
        encoder.cameraHandler.post {
            if (encoder.previewSurfaceTexture !== expectedPreviewSurfaceTexture) {
                return@post
            }
            val state = encoder.lifecycle.state
            if (state != CaptureState.Opening && state != CaptureState.Previewing) {
                return@post
            }
            val camera = encoder.cameraDevice ?: return@post
            if (encoder.encodingCompositor == null) {
                return@post
            }
            val codecGenerationSnapshot = encoder.lifecycle.codecGeneration.get()
            rebuildCaptureSession(camera, codecGenerationSnapshot)
        }
    }

    fun startPreview() {
        if (!encoder.lifecycle.compareAndSetState(CaptureState.Idle, CaptureState.Opening)) return
        // Codec-only capture is allowed so STREAMING can encode before the
        // local preview SurfaceTexture is ready (REQ-PICOO-MEDIA-001).
        encoder.lastError = null
        // CameraManager/OEM calls stay on the camera thread. Some vendor Camera2
        // implementations throw synchronously while enumerating/opening cameras;
        // never let that exception terminate the Compose main thread.
        encoder.cameraHandler.post {
            if (encoder.lifecycle.state == CaptureState.Opening) openCamera()
        }
    }

    fun restartPreviewAfterCameraCloses() {
        val generation = encoder.lifecycle.cameraGeneration.get()
        val camera = synchronized(encoder.lifecycle.cameraLifecycleLock) {
            encoder.cameraDevice.also { encoder.cameraDevice = null }
        }
        if (camera == null) {
            val previousState = encoder.lifecycle.state
            if ((previousState != CaptureState.Opening &&
                    previousState != CaptureState.Previewing) ||
                !encoder.lifecycle.compareAndSetState(previousState, CaptureState.Idle)
            ) {
                return
            }
            startPreview()
            return
        }
        encoder.lifecycle.reopenAfterCameraGeneration.set(generation)
        encoder.lifecycle.setState(CaptureState.Opening)
        closeCaptureSession()
        encoder.h264Encoder.release()
        encoder.h264Encoder.resetCounters()
        camera.close()
    }

    fun restartOpeningPreviewIfCameraOpened() {
        if (encoder.lifecycle.state == CaptureState.Opening && encoder.cameraDevice != null) {
            restartPreviewAfterCameraCloses()
        }
    }

    fun openCamera() {
        try {
            if (
                ContextCompat.checkSelfPermission(encoder.appContext, Manifest.permission.CAMERA) !=
                PackageManager.PERMISSION_GRANTED
            ) {
                encoder.fail("Camera permission is required")
                return
            }
            val cameraId = findCameraId(encoder.profile.lensFacing) ?: run {
                encoder.fail("No camera for ${encoder.profile.lensFacing}")
                return
            }
            encoder.selectedCameraId = cameraId
            encoder.activePhysicalCameraId = null
            encoder.captureSize = chooseCaptureSize(cameraId, encoder.profile.resolution)
            refreshPreviewTransformInfo()
            synchronized(encoder.lifecycle.outputSurfaceLock) {
                val surfaceTexture = encoder.previewSurfaceTexture
                if (surfaceTexture != null) {
                    runCatching {
                        surfaceTexture.setDefaultBufferSize(
                            encoder.captureSize.width,
                            encoder.captureSize.height,
                        )
                    }.onFailure {
                        // A disappearing local TextureView must not terminate the
                        // remote encoder. Drop only the invalid preview target;
                        // the next SurfaceTexture callback will bind a fresh one.
                        if (encoder.previewSurfaceTexture === surfaceTexture) {
                            encoder.previewSurfaceTexture = null
                            encoder.previewSurface?.release()
                            encoder.previewSurface = null
                        }
                        encoder.lastError = "Preview surface resize failed: ${it.message}"
                    }
                }
            }
            refreshExposureRange(cameraId)
            val openingProfile = encoder.profile
            val openingStreamEpoch = encoder.streamEpoch
            val generation = encoder.lifecycle.cameraGeneration.incrementAndGet()
            encoder.cameraManager.openCamera(
                cameraId,
                createCameraStateCallback(generation, openingProfile, openingStreamEpoch),
                encoder.cameraHandler,
            )
        } catch (error: Exception) {
            encoder.fail("openCamera failed: ${error.message ?: error.javaClass.simpleName}")
        }
    }

    fun refreshExposureRange(cameraId: String) {
        val characteristics = runCatching {
            encoder.cameraManager.getCameraCharacteristics(cameraId)
        }.getOrNull() ?: run {
            encoder.exposureCompensationRange = IntRange.EMPTY
            return
        }
        val range = characteristics.get(CameraCharacteristics.CONTROL_AE_COMPENSATION_RANGE)
        encoder.exposureCompensationRange = if (range == null) {
            IntRange.EMPTY
        } else {
            range.lower..range.upper
        }
        if (!encoder.exposureCompensationRange.isEmpty()) {
            encoder.exposureCompensation = encoder.exposureCompensation.coerceIn(
                encoder.exposureCompensationRange.first,
                encoder.exposureCompensationRange.last,
            )
        }
    }

    private fun createCameraStateCallback(
        generation: Long,
        openingProfile: CaptureProfile,
        openingStreamEpoch: Int,
    ) = object : CameraDevice.StateCallback() {
        private var reopenAfterClose = false

        override fun onOpened(camera: CameraDevice) {
            synchronized(encoder.lifecycle.cameraLifecycleLock) {
                if (generation != encoder.lifecycle.cameraGeneration.get()) {
                    camera.close()
                    return
                }
                if (encoder.profile != openingProfile || encoder.streamEpoch != openingStreamEpoch) {
                    reopenAfterClose = true
                    camera.close()
                    return
                }
                encoder.cameraDevice = camera
            }
            encoder.h264Encoder.setupEncoderAndSession(camera, generation)
        }

        override fun onClosed(camera: CameraDevice) {
            val plannedReopen =
                encoder.lifecycle.reopenAfterCameraGeneration.compareAndSet(generation, -1)
            if ((reopenAfterClose || plannedReopen) &&
                generation == encoder.lifecycle.cameraGeneration.get() &&
                encoder.lifecycle.compareAndSetState(CaptureState.Opening, CaptureState.Idle)
            ) {
                startPreview()
            }
        }

        override fun onDisconnected(camera: CameraDevice) {
            camera.close()
            if (generation != encoder.lifecycle.cameraGeneration.get()) return
            encoder.cameraDevice = null
            encoder.fail("Camera disconnected")
        }

        override fun onError(camera: CameraDevice, error: Int) {
            camera.close()
            if (generation != encoder.lifecycle.cameraGeneration.get()) return
            encoder.cameraDevice = null
            encoder.fail("Camera error $error")
        }
    }

    /** Create / replace Camera2 session using preview + compositor OES input. */
    fun rebuildCaptureSession(
        camera: CameraDevice,
        codecGenerationSnapshot: Long = encoder.lifecycle.codecGeneration.get(),
    ) {
        closeCaptureSession()
        val sessionGeneration = encoder.lifecycle.captureSessionGeneration.incrementAndGet()
        var encodingSurfaceMissing = false
        runCatching {
            synchronized(encoder.lifecycle.outputSurfaceLock) {
                if (sessionGeneration != encoder.lifecycle.captureSessionGeneration.get() ||
                    codecGenerationSnapshot != encoder.lifecycle.codecGeneration.get() ||
                    camera !== encoder.cameraDevice ||
                    encoder.lifecycle.state == CaptureState.Idle
                ) {
                    return@synchronized
                }
                val encodingTarget = encoder.encodingCompositor?.cameraInputSurface ?: run {
                    encodingSurfaceMissing = true
                    return@synchronized
                }
                val previewTarget = encoder.previewSurface
                val targets = buildList {
                    previewTarget?.let { surface ->
                        add(
                            OutputConfiguration(surface).apply {
                                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                                    setMirrorMode(OutputConfiguration.MIRROR_MODE_NONE)
                                }
                            },
                        )
                    }
                    add(
                        OutputConfiguration(encodingTarget).apply {
                            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                                setMirrorMode(OutputConfiguration.MIRROR_MODE_NONE)
                            }
                        },
                    )
                }
                val callback = object : CameraCaptureSession.StateCallback() {
                    override fun onConfigured(session: CameraCaptureSession) {
                        val accepted = synchronized(encoder.lifecycle.cameraLifecycleLock) {
                            if (sessionGeneration == encoder.lifecycle.captureSessionGeneration.get() &&
                                codecGenerationSnapshot == encoder.lifecycle.codecGeneration.get() &&
                                camera === encoder.cameraDevice
                            ) {
                                encoder.captureSession = session
                                true
                            } else {
                                false
                            }
                        }
                        if (!accepted) {
                            session.close()
                            return
                        }
                        val request = buildCaptureRequest(
                            camera = camera,
                            previewTarget = previewTarget,
                            encodingTarget = encodingTarget,
                        ).getOrElse { error ->
                            session.close()
                            if (isCurrentCaptureSession(
                                    sessionGeneration,
                                    codecGenerationSnapshot,
                                    camera,
                                )
                            ) {
                                encoder.fail("CaptureRequest failed: ${error.message}")
                            }
                            return
                        }
                        runCatching {
                            session.setRepeatingRequest(
                                request,
                                createCaptureCallback(
                                    sessionGeneration,
                                    codecGenerationSnapshot,
                                    camera,
                                ),
                                encoder.cameraHandler,
                            )
                        }.onSuccess {
                            if (sessionGeneration == encoder.lifecycle.captureSessionGeneration.get() &&
                                codecGenerationSnapshot == encoder.lifecycle.codecGeneration.get() &&
                                camera === encoder.cameraDevice
                            ) {
                                encoder.lifecycle.setState(CaptureState.Previewing)
                                encoder.h264Encoder.requestSyncFrame()
                            } else {
                                session.close()
                            }
                        }.onFailure {
                            session.close()
                            if (sessionGeneration == encoder.lifecycle.captureSessionGeneration.get() &&
                                codecGenerationSnapshot == encoder.lifecycle.codecGeneration.get() &&
                                camera === encoder.cameraDevice
                            ) {
                                encoder.fail("Capture session start failed: ${it.message}")
                            }
                        }
                    }

                    override fun onConfigureFailed(session: CameraCaptureSession) {
                        session.close()
                        if (sessionGeneration == encoder.lifecycle.captureSessionGeneration.get() &&
                            codecGenerationSnapshot == encoder.lifecycle.codecGeneration.get() &&
                            camera === encoder.cameraDevice
                        ) {
                            encoder.fail("Capture session configure failed")
                        }
                    }
                }
                camera.createCaptureSession(
                    SessionConfiguration(
                        SessionConfiguration.SESSION_REGULAR,
                        targets,
                        Executor { command -> encoder.cameraHandler.post(command) },
                        callback,
                    ),
                )
            }
        }.onFailure {
            if (sessionGeneration == encoder.lifecycle.captureSessionGeneration.get() &&
                codecGenerationSnapshot == encoder.lifecycle.codecGeneration.get() &&
                camera === encoder.cameraDevice
            ) {
                encoder.fail("createCaptureSession failed: ${it.message}")
            }
        }
        if (encodingSurfaceMissing &&
            sessionGeneration == encoder.lifecycle.captureSessionGeneration.get() &&
            codecGenerationSnapshot == encoder.lifecycle.codecGeneration.get() &&
            camera === encoder.cameraDevice &&
            encoder.lifecycle.state != CaptureState.Idle
        ) {
            encoder.fail("Encoding compositor input surface missing")
        }
    }

    private fun buildCaptureRequest(
        camera: CameraDevice,
        previewTarget: Surface?,
        encodingTarget: Surface,
    ): Result<CaptureRequest> = runCatching {
            camera.createCaptureRequest(CameraDevice.TEMPLATE_RECORD).apply {
                previewTarget?.let { addTarget(it) }
                addTarget(encodingTarget)
                set(CaptureRequest.CONTROL_AF_MODE, CaptureRequest.CONTROL_AF_MODE_CONTINUOUS_VIDEO)
                set(
                    CaptureRequest.CONTROL_AE_TARGET_FPS_RANGE,
                    Range(encoder.profile.targetFps, encoder.profile.targetFps),
                )
                if (!encoder.exposureCompensationRange.isEmpty()) {
                    set(
                        CaptureRequest.CONTROL_AE_EXPOSURE_COMPENSATION,
                        encoder.exposureCompensation,
                    )
                }
            }.build()
        }

    private fun isCurrentCaptureSession(
        sessionGeneration: Long,
        codecGenerationSnapshot: Long,
        camera: CameraDevice,
    ): Boolean = encoder.lifecycle.isCurrentCaptureSession(
        sessionGeneration,
        codecGenerationSnapshot,
        camera,
        encoder.cameraDevice,
    )

    private fun createCaptureCallback(
        sessionGeneration: Long,
        codecGenerationSnapshot: Long,
        camera: CameraDevice,
    ) = object : CameraCaptureSession.CaptureCallback() {
        override fun onCaptureCompleted(
            session: CameraCaptureSession,
            request: CaptureRequest,
            result: TotalCaptureResult,
        ) {
            if (!isCurrentCaptureSession(
                    sessionGeneration,
                    codecGenerationSnapshot,
                    camera,
                )
            ) {
                return
            }
            val physicalCameraId =
                result.get(CaptureResult.LOGICAL_MULTI_CAMERA_ACTIVE_PHYSICAL_ID)
                    ?.takeIf(String::isNotEmpty)
                    ?: return
            if (encoder.activePhysicalCameraId != physicalCameraId) {
                encoder.activePhysicalCameraId = physicalCameraId
                refreshPreviewTransformInfo()
            }
        }
    }

    fun reissueRepeatingRequest() {
        val camera = encoder.cameraDevice ?: return
        val session = encoder.captureSession ?: return
        val previewTarget = encoder.previewSurface
        val encodingTarget = encoder.encodingCompositor?.cameraInputSurface ?: return
        val request = buildCaptureRequest(camera, previewTarget, encodingTarget).getOrElse {
            encoder.lastError = "exposure request failed: ${it.message}"
            return
        }
        val sessionGeneration = encoder.lifecycle.captureSessionGeneration.get()
        val codecGenerationSnapshot = encoder.lifecycle.codecGeneration.get()
        runCatching {
            session.setRepeatingRequest(
                request,
                createCaptureCallback(
                    sessionGeneration,
                    codecGenerationSnapshot,
                    camera,
                ),
                encoder.cameraHandler,
            )
        }.onFailure {
            encoder.lastError = "exposure update failed: ${it.message}"
        }
    }

    /** Refresh dynamic camera orientation (API 32+ fold state / display reconfiguration). */
    fun refreshPreviewTransformInfo(): PreviewTransformInfo {
        val cameraId = encoder.activePhysicalCameraId ?: encoder.selectedCameraId
            ?: return encoder.previewTransformInfo
        val characteristics = runCatching {
            encoder.cameraManager.getCameraCharacteristics(cameraId)
        }.getOrNull() ?: return encoder.previewTransformInfo
        val facing = when (characteristics.get(CameraCharacteristics.LENS_FACING)) {
            CameraCharacteristics.LENS_FACING_FRONT -> LensFacing.Front
            CameraCharacteristics.LENS_FACING_BACK -> LensFacing.Back
            else -> encoder.profile.lensFacing
        }
        return PreviewTransformInfo(
            bufferSize = encoder.captureSize,
            sensorOrientationDegrees =
                characteristics.get(CameraCharacteristics.SENSOR_ORIENTATION) ?: 0,
            lensFacing = facing,
        ).also {
            encoder.previewTransformInfo = it
            encoder.encodingCompositor?.updateRotation(encoder.currentEncodingRotationDegrees())
        }
    }

    fun findCameraId(facing: LensFacing): String? {
        val target = when (facing) {
            LensFacing.Back -> CameraCharacteristics.LENS_FACING_BACK
            LensFacing.Front -> CameraCharacteristics.LENS_FACING_FRONT
        }
        return encoder.cameraManager.cameraIdList.firstOrNull { id ->
            encoder.cameraManager.getCameraCharacteristics(id)
                .get(CameraCharacteristics.LENS_FACING) == target
        }
    }

    fun chooseCaptureSize(cameraId: String, target: Size): Size {
        val characteristics = encoder.cameraManager.getCameraCharacteristics(cameraId)
        val map = characteristics
            .get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP)
            ?: return target
        val maxFrameDurationNanos = 1_000_000_000L / encoder.profile.targetFps.coerceAtLeast(1)
        val outputSizes = map.getOutputSizes(SurfaceTexture::class.java).orEmpty()
        val frameRateCapable = outputSizes.filter { size ->
            val duration = map.getOutputMinFrameDuration(SurfaceTexture::class.java, size)
            duration <= 0L || duration <= maxFrameDurationNanos
        }.ifEmpty { outputSizes.toList() }
        val sensorOrientation =
            characteristics.get(CameraCharacteristics.SENSOR_ORIENTATION) ?: 0
        val portraitCrop = StreamOrientation.relativeRotationDegrees(
            sensorOrientationDegrees = sensorOrientation,
            displayRotationDegrees = encoder.displayRotationDegrees,
            frontFacing = encoder.profile.lensFacing == LensFacing.Front,
        ) % 180 != 0
        val choices = frameRateCapable
            .map { CaptureSizeSelector.Dim(it.width, it.height) }
        val selected = CaptureSizeSelector.select(
            choices,
            CaptureSizeSelector.Dim(target.width, target.height),
            portraitCrop = portraitCrop,
        )
        if (selected.fellBackFrom1080) {
            val encode = CaptureSizeSelector.encodeSizeFor(
                selected,
                CaptureSizeSelector.Dim(target.width, target.height),
            )
            val encodeSize = Size(encode.width, encode.height)
            if (encoder.profile.resolution != encodeSize) {
                encoder.profile = encoder.profile.copy(resolution = encodeSize)
            }
        }
        return Size(selected.size.width, selected.size.height)
    }

    fun closeCaptureSession() {
        encoder.lifecycle.captureSessionGeneration.incrementAndGet()
        synchronized(encoder.lifecycle.cameraLifecycleLock) {
            runCatching { encoder.captureSession?.close() }
            encoder.captureSession = null
        }
    }

    fun closeCameraDevice() {
        synchronized(encoder.lifecycle.cameraLifecycleLock) {
            encoder.lifecycle.cameraGeneration.incrementAndGet()
            runCatching { encoder.cameraDevice?.close() }
            encoder.cameraDevice = null
        }
    }
}
