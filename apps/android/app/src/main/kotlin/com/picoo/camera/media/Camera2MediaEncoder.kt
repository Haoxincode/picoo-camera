package com.picoo.camera.media

import android.content.Context
import android.graphics.SurfaceTexture
import android.hardware.camera2.CameraCaptureSession
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraDevice
import android.hardware.camera2.CameraManager
import android.hardware.camera2.CaptureRequest
import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.os.Handler
import android.os.HandlerThread
import android.util.Range
import android.util.Size
import android.view.Surface
import java.io.Closeable
import java.util.concurrent.atomic.AtomicReference
import kotlin.math.abs

/**
 * Camera2 capture session → MediaCodec InputSurface hardware H.264 (REQ-PICOO-MEDIA-001).
 */
class Camera2MediaEncoder(
    context: Context,
    initialProfile: CaptureProfile = CaptureProfile(),
    private val frameListener: EncodedFrameListener = EncodedFrameListener.NOOP,
    private val parameterSetsListener: ParameterSetsListener = ParameterSetsListener.NOOP,
) : CameraCaptureController, Closeable {
    private val appContext = context.applicationContext
    private val cameraManager = appContext.getSystemService(CameraManager::class.java)

    override var profile: CaptureProfile = initialProfile
        private set

    override var streamEpoch: Int = StreamEpoch.INITIAL
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

    private var previewSurface: Surface? = null
    private var cameraDevice: CameraDevice? = null
    private var captureSession: CameraCaptureSession? = null
    private var mediaCodec: MediaCodec? = null
    private var codecInputSurface: Surface? = null
    private var selectedCameraId: String? = null
    private var captureSize: Size = profile.resolution

    private var frameCount = 0
    private var keyFrameCount = 0
    private var bytesSinceLastEstimate = 0L
    private var lastEstimateAtMs = System.currentTimeMillis()
    private var targetBitrateBps: Int = bitrateFor(profile.resolution)
    private var lastAppliedBitrateBps: Int = targetBitrateBps

    var stats: EncoderStats = EncoderStats()
        private set

    var lastError: String? = null
        private set

    var lastSps: ByteArray? = null
        private set
    var lastPps: ByteArray? = null
        private set

    override fun setTargetBitrateBps(bitrateBps: Int) {
        if (bitrateBps <= 0) return
        val clamped = MediaBitrate.clampAdaptive(bitrateBps, profile.resolution.height)
        targetBitrateBps = clamped
        applyBitrateIfNeeded()
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
        previewSurface = surface
        if (_state.get() == CaptureState.Idle) {
            startPreview()
        }
    }

    override fun unbindPreviewSurface() {
        previewSurface = null
        stopPreview()
    }

    override fun startPreview() {
        if (_state.get() == CaptureState.Previewing) return
        if (previewSurface == null) return
        lastError = null
        openCamera()
    }

    override fun stopPreview() {
        _state.set(CaptureState.Idle)
        closeCaptureSession()
        closeCameraDevice()
        releaseCodec()
        frameCount = 0
        keyFrameCount = 0
        stats = EncoderStats()
    }

    override fun switchCamera() {
        profile = profile.copy(
            lensFacing = when (profile.lensFacing) {
                LensFacing.Back -> LensFacing.Front
                LensFacing.Front -> LensFacing.Back
            },
        )
        streamEpoch = StreamEpoch.bump(streamEpoch)
        if (_state.get() == CaptureState.Previewing) {
            stopPreview()
            startPreview()
        }
        // New epoch requires IDR for remote decoder recovery (REQ-PICOO-MEDIA-003).
        requestSyncFrame()
    }

    override fun setResolution(width: Int, height: Int) {
        val next = Size(width, height)
        if (!StreamEpoch.shouldBumpForResolution(
                profile.resolution.width,
                profile.resolution.height,
                next.width,
                next.height,
            )
        ) {
            return
        }
        profile = profile.copy(resolution = next)
        streamEpoch = StreamEpoch.bump(streamEpoch)
        if (_state.get() == CaptureState.Previewing) {
            stopPreview()
            startPreview()
        }
        requestSyncFrame()
    }

    override fun close() {
        stopPreview()
        cameraThread.quitSafely()
        codecThread.quitSafely()
    }

    private fun openCamera() {
        val cameraId = findCameraId(profile.lensFacing) ?: run {
            fail("No camera for ${profile.lensFacing}")
            return
        }
        selectedCameraId = cameraId
        captureSize = chooseCaptureSize(cameraId, profile.resolution)
        refreshExposureRange(cameraId)

        runCatching {
            cameraManager.openCamera(cameraId, cameraStateCallback, cameraHandler)
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

    private val cameraStateCallback = object : CameraDevice.StateCallback() {
        override fun onOpened(camera: CameraDevice) {
            cameraDevice = camera
            setupEncoderAndSession(camera)
        }

        override fun onDisconnected(camera: CameraDevice) {
            camera.close()
            cameraDevice = null
            _state.set(CaptureState.Idle)
        }

        override fun onError(camera: CameraDevice, error: Int) {
            camera.close()
            cameraDevice = null
            fail("Camera error $error")
        }
    }

    private fun setupEncoderAndSession(camera: CameraDevice) {
        releaseCodec()

        val codec = createEncoder(captureSize) ?: run {
            fail("No H.264 hardware encoder")
            return
        }
        mediaCodec = codec
        codecInputSurface = codec.createInputSurface()
        codec.setCallback(codecCallback, codecHandler)
        codec.start()

        val preview = previewSurface ?: run {
            fail("Preview surface missing")
            return
        }

        val targets = listOf(preview, codecInputSurface!!)
        runCatching {
            camera.createCaptureSession(
                targets,
                object : CameraCaptureSession.StateCallback() {
                    override fun onConfigured(session: CameraCaptureSession) {
                        captureSession = session
                        val request = buildCaptureRequest(camera, session) ?: return
                        session.setRepeatingRequest(request, null, cameraHandler)
                        _state.set(CaptureState.Previewing)
                        requestSyncFrame()
                    }

                    override fun onConfigureFailed(session: CameraCaptureSession) {
                        fail("Capture session configure failed")
                    }
                },
                cameraHandler,
            )
        }.onFailure { fail("createCaptureSession failed: ${it.message}") }
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

    private val codecCallback = object : MediaCodec.Callback() {
        override fun onInputBufferAvailable(codec: MediaCodec, index: Int) {
            // InputSurface mode: camera feeds encoder directly.
        }

        override fun onOutputBufferAvailable(
            codec: MediaCodec,
            index: Int,
            info: MediaCodec.BufferInfo,
        ) {
            if (info.size <= 0) {
                codec.releaseOutputBuffer(index, false)
                return
            }
            val buffer = codec.getOutputBuffer(index) ?: run {
                codec.releaseOutputBuffer(index, false)
                return
            }
            val data = ByteArray(info.size)
            buffer.position(info.offset)
            buffer.limit(info.offset + info.size)
            buffer.get(data)
            codec.releaseOutputBuffer(index, false)

            if (info.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG != 0) {
                handleCodecConfig(data)
                return
            }

            val keyFrame = info.flags and MediaCodec.BUFFER_FLAG_KEY_FRAME != 0
            frameCount += 1
            if (keyFrame) keyFrameCount += 1
            bytesSinceLastEstimate += info.size
            updateBitrateEstimate()
            stats = EncoderStats(frameCount, keyFrameCount, stats.lastBitrateEstimateKbps)
            frameListener.onEncodedFrame(data, keyFrame, info.presentationTimeUs, streamEpoch)
        }

        override fun onError(codec: MediaCodec, e: MediaCodec.CodecException) {
            fail("MediaCodec error: ${e.diagnosticInfo}")
        }

        override fun onOutputFormatChanged(codec: MediaCodec, format: MediaFormat) {
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
        // Prefer Rust Annex-B / AVCC extractor via JNI when linked; fall back to local split.
        val extracted = runCatching {
            com.picoo.camera.jni.PicooNative.extractSpsPps(data)
        }.getOrNull()
        if (extracted != null && extracted.size == 2) {
            publishParameterSets(extracted[0], extracted[1])
            return
        }
        // Local Annex-B fallback if native lib unavailable in unit tests.
        val nals = splitAnnexB(data)
        val sps = nals.firstOrNull { it.isNotEmpty() && (it[0].toInt() and 0x1f) == 7 }
        val pps = nals.firstOrNull { it.isNotEmpty() && (it[0].toInt() and 0x1f) == 8 }
        if (sps != null && pps != null) {
            publishParameterSets(sps, pps)
        }
    }

    private fun publishParameterSets(sps: ByteArray, pps: ByteArray) {
        lastSps = sps
        lastPps = pps
        parameterSetsListener.onParameterSets(sps, pps)
    }

    private fun splitAnnexB(data: ByteArray): List<ByteArray> {
        data class Mark(val codeStart: Int, val payloadStart: Int)
        val marks = mutableListOf<Mark>()
        var i = 0
        while (i + 3 <= data.size) {
            when {
                i + 4 <= data.size &&
                    data[i] == 0.toByte() && data[i + 1] == 0.toByte() &&
                    data[i + 2] == 0.toByte() && data[i + 3] == 1.toByte() -> {
                    marks.add(Mark(i, i + 4))
                    i += 4
                }
                data[i] == 0.toByte() && data[i + 1] == 0.toByte() && data[i + 2] == 1.toByte() -> {
                    marks.add(Mark(i, i + 3))
                    i += 3
                }
                else -> i += 1
            }
        }
        return marks.mapIndexed { idx, mark ->
            val end = marks.getOrNull(idx + 1)?.codeStart ?: data.size
            data.copyOfRange(mark.payloadStart, end)
        }
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
        runCatching {
            mediaCodec?.setParameters(
                android.os.Bundle().apply {
                    putInt(MediaCodec.PARAMETER_KEY_REQUEST_SYNC_FRAME, 0)
                },
            )
        }
    }

    private fun applyBitrateIfNeeded() {
        if (targetBitrateBps == lastAppliedBitrateBps) return
        val codec = mediaCodec ?: return
        runCatching {
            codec.setParameters(
                android.os.Bundle().apply {
                    putInt(MediaCodec.PARAMETER_KEY_VIDEO_BITRATE, targetBitrateBps)
                },
            )
            lastAppliedBitrateBps = targetBitrateBps
        }
    }

    private fun createEncoder(size: Size): MediaCodec? {
        targetBitrateBps = bitrateFor(size)
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
        return runCatching {
            MediaCodec.createEncoderByType(MediaFormat.MIMETYPE_VIDEO_AVC).apply {
                configure(format, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
            }
        }.getOrNull()
    }

    fun sensorOrientationDegrees(): Int {
        val cameraId = selectedCameraId ?: return 0
        return runCatching {
            cameraManager
                .getCameraCharacteristics(cameraId)
                .get(CameraCharacteristics.SENSOR_ORIENTATION)
        }.getOrNull() ?: 0
    }

    private fun bitrateFor(size: Size): Int = MediaBitrate.forResolution(size.width, size.height)

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
        val choices = map.getOutputSizes(SurfaceTexture::class.java)?.toList().orEmpty()
        if (choices.isEmpty()) return target
        return choices.minByOrNull { size ->
            abs(size.width - target.width) + abs(size.height - target.height)
        } ?: target
    }

    private fun closeCaptureSession() {
        captureSession?.close()
        captureSession = null
    }

    private fun closeCameraDevice() {
        cameraDevice?.close()
        cameraDevice = null
    }

    private fun releaseCodec() {
        runCatching { mediaCodec?.stop() }
        runCatching { mediaCodec?.release() }
        mediaCodec = null
        codecInputSurface?.release()
        codecInputSurface = null
    }

    private fun fail(message: String) {
        lastError = message
        _state.set(CaptureState.Error)
        stopPreview()
    }
}
