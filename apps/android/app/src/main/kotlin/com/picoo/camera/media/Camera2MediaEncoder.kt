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
) : CameraCaptureController, Closeable {
    private val appContext = context.applicationContext
    private val cameraManager = appContext.getSystemService(CameraManager::class.java)

    override var profile: CaptureProfile = initialProfile
        private set

    override var streamEpoch: Int = 1
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

    var stats: EncoderStats = EncoderStats()
        private set

    var lastError: String? = null
        private set

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
        streamEpoch += 1
        if (_state.get() == CaptureState.Previewing) {
            stopPreview()
            startPreview()
        }
    }

    override fun setResolution(width: Int, height: Int) {
        val next = Size(width, height)
        if (profile.resolution == next) return
        profile = profile.copy(resolution = next)
        streamEpoch += 1
        if (_state.get() == CaptureState.Previewing) {
            stopPreview()
            startPreview()
        }
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

        runCatching {
            cameraManager.openCamera(cameraId, cameraStateCallback, cameraHandler)
        }.onFailure { fail("openCamera failed: ${it.message}") }
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
            }.build()
        }.getOrElse {
            fail("CaptureRequest failed: ${it.message}")
            null
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

        override fun onOutputFormatChanged(codec: MediaCodec, format: MediaFormat) = Unit
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

    private fun createEncoder(size: Size): MediaCodec? {
        val format = MediaFormat.createVideoFormat(MediaFormat.MIMETYPE_VIDEO_AVC, size.width, size.height).apply {
            setInteger(MediaFormat.KEY_COLOR_FORMAT, MediaCodecInfo.CodecCapabilities.COLOR_FormatSurface)
            setInteger(MediaFormat.KEY_BIT_RATE, bitrateFor(size))
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

    private fun bitrateFor(size: Size): Int {
        val pixels = size.width * size.height
        return if (pixels >= 1920 * 1080) 6_000_000 else 3_000_000
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
