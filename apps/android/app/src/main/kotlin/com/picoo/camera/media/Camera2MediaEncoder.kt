package com.picoo.camera.media

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.graphics.SurfaceTexture
import android.hardware.camera2.CameraCaptureSession
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraDevice
import android.hardware.camera2.CameraManager
import android.hardware.camera2.CaptureRequest
import android.hardware.camera2.params.OutputConfiguration
import android.hardware.camera2.params.SessionConfiguration
import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.os.Handler
import android.os.HandlerThread
import android.util.Range
import android.util.Size
import android.view.Surface
import androidx.core.content.ContextCompat
import java.io.Closeable
import java.util.concurrent.Executor
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicReference

/**
 * Camera2 capture session → MediaCodec InputSurface hardware H.264 (REQ-PICOO-MEDIA-001).
 */
class Camera2MediaEncoder(
    context: Context,
    initialProfile: CaptureProfile = CaptureProfile(),
    initialBitrateBps: Int,
    initialStreamEpoch: Int,
    private val frameListener: EncodedFrameListener = EncodedFrameListener.NOOP,
    private val parameterSetsListener: ParameterSetsListener = ParameterSetsListener.NOOP,
) : CameraCaptureController, Closeable {
    private val appContext = context.applicationContext
    private val cameraManager = appContext.getSystemService(CameraManager::class.java)

    @Volatile
    override var profile: CaptureProfile = initialProfile
        private set

    @Volatile
    override var streamEpoch: Int = initialStreamEpoch
        private set

    override var exposureCompensation: Int = 0
        private set

    override var exposureCompensationRange: IntRange = IntRange.EMPTY
        private set

    private val _state = AtomicReference(CaptureState.Idle)
    override val state: CaptureState
        get() = _state.get()

    private val cameraThread = HandlerThread("picoo-camera").apply { start() }
    private val cameraHandler = Handler(cameraThread.looper)
    private val codecThread = HandlerThread("picoo-codec").apply { start() }
    private val codecHandler = Handler(codecThread.looper)

    @Volatile private var previewSurface: Surface? = null
    @Volatile private var cameraDevice: CameraDevice? = null
    @Volatile private var captureSession: CameraCaptureSession? = null
    @Volatile private var mediaCodec: MediaCodec? = null
    @Volatile private var codecInputSurface: Surface? = null
    @Volatile private var selectedCameraId: String? = null
    private var captureSize: Size = profile.resolution

    private var frameCount = 0
    private var keyFrameCount = 0
    private var bytesSinceLastEstimate = 0L
    private var lastEstimateAtMs = System.currentTimeMillis()
    @Volatile private var targetBitrateBps: Int = initialBitrateBps
    @Volatile private var lastAppliedBitrateBps: Int = targetBitrateBps
    private val cameraGeneration = AtomicLong(0)
    private val codecGeneration = AtomicLong(0)
    private val cameraLifecycleLock = Any()
    private val codecLifecycleLock = Any()

    private data class DetachedCodec(
        val codec: MediaCodec?,
        val surface: Surface?,
        val nextGeneration: Long,
    )

    @Volatile
    var appliedStreamEpoch: Int = 0
        private set

    @Volatile
    var appliedEncoderHeight: Int = 0
        private set

    var stats: EncoderStats = EncoderStats()
        private set

    @Volatile
    var lastError: String? = null
        private set

    var lastSps: ByteArray? = null
        private set
    var lastPps: ByteArray? = null
        private set

    override fun setTargetBitrateBps(bitrateBps: Int) {
        if (bitrateBps <= 0) return
        targetBitrateBps = bitrateBps
        applyBitrateIfNeeded()
    }

    override fun prepareStreamEpoch(epoch: Int) {
        require(epoch > 0) { "stream epoch must come from Rust" }
        streamEpoch = epoch
    }

    override fun requestKeyFrame() {
        requestSyncFrame()
    }

    override fun setExposureCompensation(index: Int) {
        val clamped = ExposureCompensation.clamp(index, exposureCompensationRange)
        if (clamped == exposureCompensation) return
        exposureCompensation = clamped
        reissueRepeatingRequest()
    }

    override fun bindPreviewSurface(surface: Surface) {
        val changed = previewSurface !== surface
        previewSurface = surface
        when {
            _state.get() == CaptureState.Idle -> startPreview()
            changed && cameraDevice != null && codecInputSurface != null ->
                rebuildCaptureSession(cameraDevice!!)
        }
    }

    override fun unbindPreviewSurface() {
        if (previewSurface == null) return
        previewSurface = null
        // Keep H.264 encode alive when the Compose TextureView is torn down
        // (tab switch / config change); rebuild a codec-only Camera2 session.
        val camera = cameraDevice
        if (_state.get() == CaptureState.Previewing && camera != null && codecInputSurface != null) {
            rebuildCaptureSession(camera)
        }
    }

    override fun startPreview() {
        if (_state.get() == CaptureState.Previewing) return
        // Codec-only capture is allowed so STREAMING can encode before the
        // local preview SurfaceTexture is ready (REQ-PICOO-MEDIA-001).
        lastError = null
        openCamera()
    }

    override fun stopPreview() {
        // Invalidate callbacks before any resource begins closing.
        cameraGeneration.incrementAndGet()
        codecGeneration.incrementAndGet()
        _state.set(CaptureState.Idle)
        closeCaptureSession()
        closeCameraDevice()
        releaseCodec()
        frameCount = 0
        keyFrameCount = 0
        stats = EncoderStats()
        _state.set(CaptureState.Idle)
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
        if (_state.get() == CaptureState.Previewing) {
            stopPreview()
            startPreview()
        }
        // New epoch requires IDR for remote decoder recovery (REQ-PICOO-MEDIA-003).
        requestSyncFrame()
    }

    override fun setResolution(width: Int, height: Int) {
        val next = Size(width, height)
        profile = profile.copy(resolution = next)
        if (_state.get() == CaptureState.Previewing) {
            stopPreview()
            startPreview()
        }
        requestSyncFrame()
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
        stopPreview()
        startPreview()
    }

    override fun close() {
        stopPreview()
        cameraThread.quitSafely()
        codecThread.quitSafely()
    }

    private fun openCamera() {
        if (
            ContextCompat.checkSelfPermission(appContext, Manifest.permission.CAMERA) !=
            PackageManager.PERMISSION_GRANTED
        ) {
            fail("Camera permission is required")
            return
        }
        val cameraId = findCameraId(profile.lensFacing) ?: run {
            fail("No camera for ${profile.lensFacing}")
            return
        }
        selectedCameraId = cameraId
        captureSize = chooseCaptureSize(cameraId, profile.resolution)
        refreshExposureRange(cameraId)

        val generation = cameraGeneration.incrementAndGet()
        runCatching {
            cameraManager.openCamera(
                cameraId,
                createCameraStateCallback(generation),
                cameraHandler,
            )
        }.onFailure { fail("openCamera failed: ${it.message}") }
    }

    private fun refreshExposureRange(cameraId: String) {
        val characteristics = runCatching {
            cameraManager.getCameraCharacteristics(cameraId)
        }.getOrNull() ?: run {
            exposureCompensationRange = IntRange.EMPTY
            return
        }
        val range = characteristics.get(CameraCharacteristics.CONTROL_AE_COMPENSATION_RANGE)
        exposureCompensationRange = if (range == null) {
            IntRange.EMPTY
        } else {
            range.lower..range.upper
        }
        if (!exposureCompensationRange.isEmpty()) {
            exposureCompensation = exposureCompensation.coerceIn(
                exposureCompensationRange.first,
                exposureCompensationRange.last,
            )
        }
    }

    private fun createCameraStateCallback(generation: Long) = object : CameraDevice.StateCallback() {
        override fun onOpened(camera: CameraDevice) {
            synchronized(cameraLifecycleLock) {
                if (generation != cameraGeneration.get()) {
                    camera.close()
                    return
                }
                cameraDevice = camera
            }
            setupEncoderAndSession(camera, generation)
        }

        override fun onDisconnected(camera: CameraDevice) {
            camera.close()
            if (generation != cameraGeneration.get()) return
            cameraDevice = null
            fail("Camera disconnected")
        }

        override fun onError(camera: CameraDevice, error: Int) {
            camera.close()
            if (generation != cameraGeneration.get()) return
            cameraDevice = null
            fail("Camera error $error")
        }
    }

    private fun setupEncoderAndSession(camera: CameraDevice, cameraGenerationSnapshot: Long) {
        val encodeSize = profile.resolution
        val generationEpoch = streamEpoch
        val transition = detachCodec()
        val generation = transition.nextGeneration
        appliedStreamEpoch = 0
        appliedEncoderHeight = 0

        // MediaCodec implementations are frequently single-instance. Keep the
        // complete old-release -> new-create/configure/start transition on the
        // codec thread so two hardware encoders can never overlap.
        codecHandler.post {
            releaseCodecResources(transition)
            if (!isCurrentCodecTransition(generation, camera, cameraGenerationSnapshot)) {
                return@post
            }

            val codec = createEncoder(encodeSize) ?: run {
                reportCodecStartFailure(
                    generation,
                    camera,
                    cameraGenerationSnapshot,
                    "No H.264 hardware encoder",
                )
                return@post
            }
            var inputSurface: Surface? = null
            try {
                if (!isCurrentCodecTransition(generation, camera, cameraGenerationSnapshot)) {
                    runCatching { codec.release() }
                    return@post
                }
                inputSurface = codec.createInputSurface()
                codec.setCallback(
                    createCodecCallback(generation, generationEpoch, encodeSize.height),
                    codecHandler,
                )
                val accepted = synchronized(codecLifecycleLock) {
                    if (isCurrentCodecTransition(generation, camera, cameraGenerationSnapshot)) {
                        mediaCodec = codec
                        codecInputSurface = inputSurface
                        true
                    } else {
                        false
                    }
                }
                if (!accepted) {
                    releaseCodecResources(DetachedCodec(codec, inputSurface, generation))
                    return@post
                }
                codec.start()
            } catch (error: RuntimeException) {
                synchronized(codecLifecycleLock) {
                    if (mediaCodec === codec) mediaCodec = null
                    if (codecInputSurface === inputSurface) codecInputSurface = null
                }
                releaseCodecResources(DetachedCodec(codec, inputSurface, generation))
                reportCodecStartFailure(
                    generation,
                    camera,
                    cameraGenerationSnapshot,
                    "MediaCodec start failed: ${error.message}",
                )
                return@post
            }

            cameraHandler.post {
                if (isCurrentCodecTransition(generation, camera, cameraGenerationSnapshot)) {
                    rebuildCaptureSession(camera, generation)
                }
            }
        }
    }

    private fun isCurrentCodecTransition(
        generation: Long,
        camera: CameraDevice,
        cameraGenerationSnapshot: Long,
    ): Boolean = generation == codecGeneration.get() &&
        cameraGenerationSnapshot == cameraGeneration.get() &&
        camera === cameraDevice

    private fun reportCodecStartFailure(
        generation: Long,
        camera: CameraDevice,
        cameraGenerationSnapshot: Long,
        message: String,
    ) {
        cameraHandler.post {
            if (isCurrentCodecTransition(generation, camera, cameraGenerationSnapshot)) {
                fail(message)
            }
        }
    }

    /** Create / replace the Camera2 session using preview (optional) + codec InputSurface. */
    private fun rebuildCaptureSession(
        camera: CameraDevice,
        generation: Long = codecGeneration.get(),
    ) {
        val codecSurface = codecInputSurface ?: run {
            fail("Codec input surface missing")
            return
        }
        closeCaptureSession()
        val targets = buildList {
            previewSurface?.let { add(it) }
            add(codecSurface)
        }
        runCatching {
            val callback = object : CameraCaptureSession.StateCallback() {
                    override fun onConfigured(session: CameraCaptureSession) {
                        val accepted = synchronized(cameraLifecycleLock) {
                            if (generation == codecGeneration.get() && camera === cameraDevice) {
                                captureSession = session
                                true
                            } else {
                                false
                            }
                        }
                        if (!accepted) {
                            session.close()
                            return
                        }
                        val request = buildCaptureRequest(camera, session) ?: return
                        runCatching {
                            session.setRepeatingRequest(request, null, cameraHandler)
                        }.onSuccess {
                            if (generation == codecGeneration.get() && camera === cameraDevice) {
                                _state.set(CaptureState.Previewing)
                                requestSyncFrame()
                            } else {
                                session.close()
                            }
                        }.onFailure {
                            session.close()
                            if (generation == codecGeneration.get() && camera === cameraDevice) {
                                fail("Capture session start failed: ${it.message}")
                            }
                        }
                    }

                    override fun onConfigureFailed(session: CameraCaptureSession) {
                        session.close()
                        if (generation == codecGeneration.get() && camera === cameraDevice) {
                            fail("Capture session configure failed")
                        }
                    }
                }
            camera.createCaptureSession(
                SessionConfiguration(
                    SessionConfiguration.SESSION_REGULAR,
                    targets.map(::OutputConfiguration),
                    Executor { command -> cameraHandler.post(command) },
                    callback,
                ),
            )
        }.onFailure {
            if (generation == codecGeneration.get() && camera === cameraDevice) {
                fail("createCaptureSession failed: ${it.message}")
            }
        }
    }

    private fun buildCaptureRequest(
        camera: CameraDevice,
        session: CameraCaptureSession,
    ): CaptureRequest? {
        return runCatching {
            camera.createCaptureRequest(CameraDevice.TEMPLATE_RECORD).apply {
                previewSurface?.let { addTarget(it) }
                codecInputSurface?.let { addTarget(it) }
                set(CaptureRequest.CONTROL_AF_MODE, CaptureRequest.CONTROL_AF_MODE_CONTINUOUS_VIDEO)
                set(CaptureRequest.CONTROL_AE_TARGET_FPS_RANGE, Range(profile.targetFps, profile.targetFps))
                if (!exposureCompensationRange.isEmpty()) {
                    set(CaptureRequest.CONTROL_AE_EXPOSURE_COMPENSATION, exposureCompensation)
                }
            }.build()
        }.getOrElse {
            fail("CaptureRequest failed: ${it.message}")
            null
        }
    }

    private fun reissueRepeatingRequest() {
        val camera = cameraDevice ?: return
        val session = captureSession ?: return
        val request = buildCaptureRequest(camera, session) ?: return
        runCatching {
            session.setRepeatingRequest(request, null, cameraHandler)
        }.onFailure {
            lastError = "exposure update failed: ${it.message}"
        }
    }

    private fun createCodecCallback(
        generation: Long,
        generationEpoch: Int,
        generationHeight: Int,
    ) = object : MediaCodec.Callback() {
        override fun onInputBufferAvailable(codec: MediaCodec, index: Int) {
            // InputSurface mode: camera feeds encoder directly.
        }

        override fun onOutputBufferAvailable(
            codec: MediaCodec,
            index: Int,
            info: MediaCodec.BufferInfo,
        ) {
            val active = synchronized(codecLifecycleLock) {
                generation == codecGeneration.get() && codec === mediaCodec
            }
            if (!active) {
                runCatching { codec.releaseOutputBuffer(index, false) }
                return
            }
            if (info.size <= 0) {
                runCatching { codec.releaseOutputBuffer(index, false) }
                return
            }
            val buffer = runCatching { codec.getOutputBuffer(index) }.getOrNull() ?: run {
                runCatching { codec.releaseOutputBuffer(index, false) }
                return
            }
            val data = ByteArray(info.size)
            buffer.position(info.offset)
            buffer.limit(info.offset + info.size)
            buffer.get(data)
            runCatching { codec.releaseOutputBuffer(index, false) }.getOrElse { return }

            if (info.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG != 0) {
                handleCodecConfig(data)
                return
            }

            val keyFrame = info.flags and MediaCodec.BUFFER_FLAG_KEY_FRAME != 0
            if (keyFrame) {
                appliedStreamEpoch = generationEpoch
                appliedEncoderHeight = generationHeight
            }
            frameCount += 1
            if (keyFrame) keyFrameCount += 1
            bytesSinceLastEstimate += info.size
            updateBitrateEstimate()
            stats = EncoderStats(frameCount, keyFrameCount, stats.lastBitrateEstimateKbps)
            frameListener.onEncodedFrame(data, keyFrame, info.presentationTimeUs, generationEpoch)
        }

        override fun onError(codec: MediaCodec, e: MediaCodec.CodecException) {
            val active = synchronized(codecLifecycleLock) {
                generation == codecGeneration.get() && codec === mediaCodec
            }
            if (!active) return
            fail("MediaCodec error: ${e.diagnosticInfo}")
        }

        override fun onOutputFormatChanged(codec: MediaCodec, format: MediaFormat) {
            val active = synchronized(codecLifecycleLock) {
                generation == codecGeneration.get() && codec === mediaCodec
            }
            if (!active) return
            val csd0 = format.getByteBuffer("csd-0") ?: return
            val copy = ByteArray(csd0.remaining())
            csd0.mark()
            csd0.get(copy)
            csd0.reset()
            val csd1 = format.getByteBuffer("csd-1")
            if (csd1 != null) {
                val pps = ByteArray(csd1.remaining())
                csd1.mark()
                csd1.get(pps)
                csd1.reset()
                // csd-0 is often AVCC or raw SPS; csd-1 raw PPS.
                publishParameterSets(copy, pps)
            } else {
                handleCodecConfig(copy)
            }
        }
    }

    private fun handleCodecConfig(data: ByteArray) {
        // H.264 parameter-set parsing is protocol behavior and has one Rust implementation.
        val extracted = runCatching {
            com.picoo.camera.jni.PicooNative.extractSpsPps(data)
        }.getOrNull()
        if (extracted != null && extracted.size == 2) {
            publishParameterSets(extracted[0], extracted[1])
        }
    }

    private fun publishParameterSets(sps: ByteArray, pps: ByteArray) {
        lastSps = sps
        lastPps = pps
        parameterSetsListener.onParameterSets(sps, pps)
    }

    private fun updateBitrateEstimate() {
        val now = System.currentTimeMillis()
        val elapsed = now - lastEstimateAtMs
        if (elapsed >= 1000) {
            val kbps = ((bytesSinceLastEstimate * 8) / elapsed).toInt()
            stats = stats.copy(lastBitrateEstimateKbps = kbps)
            bytesSinceLastEstimate = 0
            lastEstimateAtMs = now
        }
    }

    private fun requestSyncFrame() {
        val codec = synchronized(codecLifecycleLock) { mediaCodec } ?: return
        codecHandler.post {
            val active = synchronized(codecLifecycleLock) { codec === mediaCodec }
            if (active) {
                runCatching {
                    codec.setParameters(
                        android.os.Bundle().apply {
                            putInt(MediaCodec.PARAMETER_KEY_REQUEST_SYNC_FRAME, 0)
                        },
                    )
                }
            }
        }
    }

    private fun applyBitrateIfNeeded() {
        if (targetBitrateBps == lastAppliedBitrateBps) return
        val codec = synchronized(codecLifecycleLock) { mediaCodec } ?: return
        val requestedBitrate = targetBitrateBps
        codecHandler.post {
            val active = synchronized(codecLifecycleLock) { codec === mediaCodec }
            if (active) {
                runCatching {
                    codec.setParameters(
                        android.os.Bundle().apply {
                            putInt(MediaCodec.PARAMETER_KEY_VIDEO_BITRATE, requestedBitrate)
                        },
                    )
                    lastAppliedBitrateBps = requestedBitrate
                }
            }
        }
    }

    private fun createEncoder(size: Size): MediaCodec? {
        lastAppliedBitrateBps = targetBitrateBps
        val format = MediaFormat.createVideoFormat(MediaFormat.MIMETYPE_VIDEO_AVC, size.width, size.height).apply {
            setInteger(MediaFormat.KEY_COLOR_FORMAT, MediaCodecInfo.CodecCapabilities.COLOR_FormatSurface)
            setInteger(MediaFormat.KEY_BIT_RATE, targetBitrateBps)
            setInteger(MediaFormat.KEY_FRAME_RATE, profile.targetFps)
            setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, 2)
            setInteger(
                MediaFormat.KEY_BITRATE_MODE,
                MediaCodecInfo.EncoderCapabilities.BITRATE_MODE_VBR,
            )
        }
        val codec = runCatching {
            MediaCodec.createEncoderByType(MediaFormat.MIMETYPE_VIDEO_AVC)
        }.getOrNull() ?: return null
        return runCatching {
            codec.configure(format, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
            codec
        }.getOrElse {
            runCatching { codec.release() }
            null
        }
    }

    fun sensorOrientationDegrees(): Int {
        val cameraId = selectedCameraId ?: return 0
        return runCatching {
            cameraManager
                .getCameraCharacteristics(cameraId)
                .get(CameraCharacteristics.SENSOR_ORIENTATION)
        }.getOrNull() ?: 0
    }

    private fun findCameraId(facing: LensFacing): String? {
        val target = when (facing) {
            LensFacing.Back -> CameraCharacteristics.LENS_FACING_BACK
            LensFacing.Front -> CameraCharacteristics.LENS_FACING_FRONT
        }
        return cameraManager.cameraIdList.firstOrNull { id ->
            cameraManager.getCameraCharacteristics(id)
                .get(CameraCharacteristics.LENS_FACING) == target
        }
    }

    private fun chooseCaptureSize(cameraId: String, target: Size): Size {
        val map = cameraManager.getCameraCharacteristics(cameraId)
            .get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP)
            ?: return target
        val choices = map.getOutputSizes(SurfaceTexture::class.java)
            ?.map { CaptureSizeSelector.Dim(it.width, it.height) }
            .orEmpty()
        val selected = CaptureSizeSelector.select(
            choices,
            CaptureSizeSelector.Dim(target.width, target.height),
        )
        if (selected.fellBackFrom1080) {
            val encode = CaptureSizeSelector.encodeSizeFor(
                selected,
                CaptureSizeSelector.Dim(target.width, target.height),
            )
            val encodeSize = Size(encode.width, encode.height)
            if (profile.resolution != encodeSize) {
                profile = profile.copy(resolution = encodeSize)
            }
        }
        return Size(selected.size.width, selected.size.height)
    }

    private fun closeCaptureSession() {
        synchronized(cameraLifecycleLock) {
            captureSession?.close()
            captureSession = null
        }
    }

    private fun closeCameraDevice() {
        synchronized(cameraLifecycleLock) {
            cameraGeneration.incrementAndGet()
            cameraDevice?.close()
            cameraDevice = null
        }
    }

    private fun releaseCodec() {
        val detached = detachCodec()
        codecHandler.post { releaseCodecResources(detached) }
    }

    private fun detachCodec(): DetachedCodec = synchronized(codecLifecycleLock) {
        val generation = codecGeneration.incrementAndGet()
        DetachedCodec(
            codec = mediaCodec.also { mediaCodec = null },
            surface = codecInputSurface.also { codecInputSurface = null },
            nextGeneration = generation,
        )
    }

    private fun releaseCodecResources(detached: DetachedCodec) {
        runCatching { detached.codec?.stop() }
        runCatching { detached.codec?.release() }
        runCatching { detached.surface?.release() }
    }

    private fun fail(message: String) {
        lastError = message
        _state.set(CaptureState.Error)
        closeCaptureSession()
        closeCameraDevice()
        releaseCodec()
    }
}
