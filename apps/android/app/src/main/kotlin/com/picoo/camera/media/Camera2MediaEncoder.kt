package com.picoo.camera.media

import android.content.Context
import android.graphics.SurfaceTexture
import android.hardware.camera2.CameraCaptureSession
import android.hardware.camera2.CameraDevice
import android.hardware.camera2.CameraManager
import android.media.MediaCodec
import android.os.Handler
import android.os.HandlerThread
import android.util.Log
import android.util.Size
import android.view.Surface
import java.io.Closeable

/** Camera2 → OES/EGL compositor → MediaCodec InputSurface H.264 (MEDIA-001 / MEDIA-013). */
class Camera2MediaEncoder(
    context: Context,
    initialProfile: CaptureProfile = CaptureProfile(),
    initialBitrateBps: Int,
    initialStreamEpoch: Int,
    internal val frameListener: EncodedFrameListener = EncodedFrameListener.NOOP,
    internal val parameterSetsListener: ParameterSetsListener = ParameterSetsListener.NOOP,
) : CameraCaptureController, Closeable {
    internal val appContext = context.applicationContext
    internal val cameraManager = appContext.getSystemService(CameraManager::class.java)
    internal val lifecycle = CaptureLifecycle()

    @Volatile
    override var profile: CaptureProfile = initialProfile
        internal set

    @Volatile
    override var previewTransformInfo = PreviewTransformInfo(
        bufferSize = initialProfile.resolution,
        lensFacing = initialProfile.lensFacing,
    )
        internal set

    @Volatile
    override var streamEpoch: Int = initialStreamEpoch
        internal set

    override var exposureCompensation: Int = 0
        internal set

    override var exposureCompensationRange: IntRange = IntRange.EMPTY
        internal set

    override val state: CaptureState
        get() = lifecycle.state

    private val cameraThread = HandlerThread("picoo-camera").apply { start() }
    internal val cameraHandler = Handler(cameraThread.looper)
    private val codecThread = HandlerThread("picoo-codec").apply { start() }
    internal val codecHandler = Handler(codecThread.looper)

    @Volatile internal var previewSurfaceTexture: SurfaceTexture? = null
    @Volatile internal var previewSurface: Surface? = null
    @Volatile internal var cameraDevice: CameraDevice? = null
    @Volatile internal var captureSession: CameraCaptureSession? = null
    @Volatile internal var mediaCodec: MediaCodec? = null
    @Volatile internal var codecInputSurface: Surface? = null
    @Volatile internal var encodingCompositor: CameraEncodingCompositor? = null
    @Volatile internal var selectedCameraId: String? = null
    @Volatile internal var activePhysicalCameraId: String? = null
    @Volatile internal var displayRotationDegrees: Int = 0
    internal var captureSize: Size = profile.resolution

    internal var frameCount = 0
    internal var keyFrameCount = 0
    internal var bytesSinceLastEstimate = 0L
    internal var lastEstimateAtMs = System.currentTimeMillis()
    @Volatile internal var targetBitrateBps: Int = initialBitrateBps
    @Volatile internal var lastAppliedBitrateBps: Int = targetBitrateBps

    internal val deviceSession = Camera2DeviceSession(this)
    internal val h264Encoder = MediaCodecH264Encoder(this)

    @Volatile
    var appliedStreamEpoch: Int = 0
        internal set

    @Volatile
    var appliedEncoderHeight: Int = 0
        internal set

    var stats: EncoderStats = EncoderStats()
        internal set

    @Volatile
    var lastError: String? = null
        internal set

    var lastSps: ByteArray? = null
        internal set
    var lastPps: ByteArray? = null
        internal set

    override fun setTargetBitrateBps(bitrateBps: Int) {
        if (bitrateBps <= 0) return
        targetBitrateBps = bitrateBps
        h264Encoder.applyBitrateIfNeeded()
    }

    override fun prepareStreamEpoch(epoch: Int) {
        require(epoch > 0) { "stream epoch must come from Rust" }
        streamEpoch = epoch
        deviceSession.restartOpeningPreviewIfCameraOpened()
    }

    override fun requestKeyFrame() {
        h264Encoder.requestSyncFrame()
    }

    override fun setExposureCompensation(index: Int) {
        val clamped = ExposureCompensation.clamp(index, exposureCompensationRange)
        if (clamped == exposureCompensation) return
        exposureCompensation = clamped
        deviceSession.reissueRepeatingRequest()
    }

    /** Update the display-relative transform used before frames enter MediaCodec. */
    fun setDisplayRotationDegrees(rotationDegrees: Int) {
        val normalized = ((rotationDegrees % 360) + 360) % 360
        if (displayRotationDegrees == normalized) return
        displayRotationDegrees = normalized
        encodingCompositor?.updateRotation(currentEncodingRotationDegrees())
    }

    override fun bindPreviewSurface(surfaceTexture: SurfaceTexture) {
        deviceSession.bindPreviewSurface(surfaceTexture)
    }

    override fun unbindPreviewSurface(surfaceTexture: SurfaceTexture) {
        deviceSession.unbindPreviewSurface(surfaceTexture)
    }

    override fun startPreview() {
        deviceSession.startPreview()
    }

    override fun stopPreview() {
        // Invalidate callbacks before any resource begins closing.
        lifecycle.reopenAfterCameraGeneration.set(-1)
        lifecycle.cameraGeneration.incrementAndGet()
        lifecycle.codecGeneration.incrementAndGet()
        lifecycle.setState(CaptureState.Idle)
        deviceSession.closeCaptureSession()
        deviceSession.closeCameraDevice()
        h264Encoder.release()
        h264Encoder.resetCounters()
        lifecycle.setState(CaptureState.Idle)
    }

    override fun switchCamera() {
        setLensFacing(
            when (profile.lensFacing) {
                LensFacing.Back -> LensFacing.Front
                LensFacing.Front -> LensFacing.Back
            },
        )
    }

    override fun setLensFacing(facing: LensFacing) {
        if (profile.lensFacing == facing) {
            return
        }
        profile = profile.copy(lensFacing = facing)
        when (lifecycle.state) {
            CaptureState.Previewing -> deviceSession.restartPreviewAfterCameraCloses()
            CaptureState.Opening -> deviceSession.restartOpeningPreviewIfCameraOpened()
            else -> Unit
        }
        // New epoch requires IDR for remote decoder recovery (REQ-PICOO-MEDIA-003).
        h264Encoder.requestSyncFrame()
    }

    override fun setResolution(width: Int, height: Int) {
        val next = Size(width, height)
        profile = profile.copy(resolution = next)
        when (lifecycle.state) {
            CaptureState.Previewing -> deviceSession.restartPreviewAfterCameraCloses()
            CaptureState.Opening -> deviceSession.restartOpeningPreviewIfCameraOpened()
            else -> Unit
        }
        h264Encoder.requestSyncFrame()
    }

    /** Rebuild the native encoder at Rust's last committed generation. */
    fun restoreCommittedConfiguration(
        width: Int,
        height: Int,
        streamEpoch: Int,
        bitrateBps: Int,
    ) {
        require(streamEpoch > 0)
        profile = profile.copy(resolution = Size(width, height))
        this.streamEpoch = streamEpoch
        targetBitrateBps = bitrateBps
        appliedStreamEpoch = 0
        appliedEncoderHeight = 0
        when (lifecycle.state) {
            CaptureState.Previewing -> deviceSession.restartPreviewAfterCameraCloses()
            CaptureState.Opening -> deviceSession.restartOpeningPreviewIfCameraOpened()
            else -> {
                stopPreview()
                startPreview()
            }
        }
    }

    override fun close() {
        stopPreview()
        synchronized(lifecycle.outputSurfaceLock) {
            previewSurfaceTexture = null
            previewSurface?.release()
            previewSurface = null
        }
        cameraThread.quitSafely()
        codecThread.quitSafely()
    }

    fun refreshPreviewTransformInfo(): PreviewTransformInfo =
        deviceSession.refreshPreviewTransformInfo()

    internal fun currentEncodingRotationDegrees(): Int =
        StreamOrientation.relativeRotationDegrees(
            sensorOrientationDegrees = previewTransformInfo.sensorOrientationDegrees,
            displayRotationDegrees = displayRotationDegrees,
            frontFacing = previewTransformInfo.lensFacing == LensFacing.Front,
        )

    internal fun fail(message: String) {
        Log.e(TAG, message)
        lastError = message
        lifecycle.setState(CaptureState.Error)
        deviceSession.closeCaptureSession()
        deviceSession.closeCameraDevice()
        h264Encoder.release()
    }

    private companion object {
        const val TAG = "Camera2MediaEncoder"
    }
}
