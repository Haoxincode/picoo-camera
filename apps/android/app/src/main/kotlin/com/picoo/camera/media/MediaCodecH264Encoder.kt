package com.picoo.camera.media

import android.hardware.camera2.CameraDevice
import android.media.MediaCodec
import android.media.MediaFormat
import android.view.Surface

/** MediaCodec H.264 access-unit lifecycle (MEDIA-001). */
internal class MediaCodecH264Encoder(
    private val encoder: Camera2MediaEncoder,
) {
    private data class DetachedCodec(
        val codec: MediaCodec?,
        val surface: Surface?,
        val compositor: CameraEncodingCompositor?,
        val nextGeneration: Long,
    )

    fun setupEncoderAndSession(camera: CameraDevice, cameraGenerationSnapshot: Long) {
        val profile = encoder.profile
        val encodeSize = profile.resolution
        val requestedBitrate = encoder.targetBitrateBps
        val request = NativeEncoderFormat(NativeVideoCodec.Avc, encodeSize, profile.targetFps, requestedBitrate)
        val generationEpoch = encoder.streamEpoch
        val transition = detachCodec()
        val generation = transition.nextGeneration
        encoder.appliedStreamEpoch = 0
        encoder.appliedEncoderHeight = 0

        // MediaCodec implementations are frequently single-instance. Keep the
        // complete old-release -> new-create/configure/start transition on the
        // codec thread so two hardware encoders can never overlap.
        encoder.codecHandler.post {
            releaseCodecResources(transition)
            if (!isCurrentCodecTransition(generation, camera, cameraGenerationSnapshot)) {
                return@post
            }

            val codec = NativeVideoEncoder.create(request).getOrElse { error ->
                reportCodecStartFailure(
                    generation,
                    camera,
                    cameraGenerationSnapshot,
                    "Hardware AVC High encoder unavailable: ${error.message}",
                )
                return@post
            }
            encoder.lastAppliedBitrateBps = requestedBitrate
            var inputSurface: Surface? = null
            var compositor: CameraEncodingCompositor? = null
            try {
                if (!isCurrentCodecTransition(generation, camera, cameraGenerationSnapshot)) {
                    runCatching { codec.release() }
                    return@post
                }
                inputSurface = codec.createInputSurface()
                compositor = CameraEncodingCompositor.create(
                    encoderSurface = inputSurface,
                    cameraBufferSize = encoder.captureSize,
                    outputSize = encodeSize,
                    initialRotationDegrees = encoder.currentEncodingRotationDegrees(),
                    onError = { message ->
                        reportCodecStartFailure(
                            generation,
                            camera,
                            cameraGenerationSnapshot,
                            message,
                        )
                    },
                ).getOrElse { error ->
                    runCatching { inputSurface.release() }
                    runCatching { codec.release() }
                    reportCodecStartFailure(
                        generation,
                        camera,
                        cameraGenerationSnapshot,
                        "Encoding compositor start failed: ${error.message}",
                    )
                    return@post
                }
                codec.setCallback(
                    createCodecCallback(
                        generation,
                        generationEpoch,
                        encodeSize.width,
                        encodeSize.height,
                        request,
                    ),
                    encoder.codecHandler,
                )
                val accepted = synchronized(encoder.lifecycle.codecLifecycleLock) {
                    if (isCurrentCodecTransition(generation, camera, cameraGenerationSnapshot)) {
                        encoder.mediaCodec = codec
                        encoder.codecInputSurface = inputSurface
                        encoder.encodingCompositor = compositor
                        true
                    } else {
                        false
                    }
                }
                if (!accepted) {
                    releaseCodecResources(
                        DetachedCodec(codec, inputSurface, compositor, generation),
                    )
                    return@post
                }
                codec.start()
            } catch (error: RuntimeException) {
                synchronized(encoder.lifecycle.codecLifecycleLock) {
                    if (encoder.mediaCodec === codec) encoder.mediaCodec = null
                    if (encoder.codecInputSurface === inputSurface) encoder.codecInputSurface = null
                    if (encoder.encodingCompositor === compositor) encoder.encodingCompositor = null
                }
                releaseCodecResources(
                    DetachedCodec(codec, inputSurface, compositor, generation),
                )
                reportCodecStartFailure(
                    generation,
                    camera,
                    cameraGenerationSnapshot,
                    "MediaCodec start failed: ${error.message}",
                )
                return@post
            }

            encoder.cameraHandler.post {
                if (isCurrentCodecTransition(generation, camera, cameraGenerationSnapshot)) {
                    encoder.deviceSession.rebuildCaptureSession(camera, generation)
                }
            }
        }
    }

    private fun isCurrentCodecTransition(
        generation: Long,
        camera: CameraDevice,
        cameraGenerationSnapshot: Long,
    ): Boolean = encoder.lifecycle.isCurrentCodecTransition(
        generation,
        camera,
        cameraGenerationSnapshot,
        encoder.cameraDevice,
    )

    private fun reportCodecStartFailure(
        generation: Long,
        camera: CameraDevice,
        cameraGenerationSnapshot: Long,
        message: String,
    ) {
        encoder.cameraHandler.post {
            if (isCurrentCodecTransition(generation, camera, cameraGenerationSnapshot)) {
                encoder.fail(message)
            }
        }
    }

    private fun createCodecCallback(
        generation: Long,
        generationEpoch: Int,
        generationWidth: Int,
        generationHeight: Int,
        request: NativeEncoderFormat,
    ) = object : MediaCodec.Callback() {
        private var formatAccepted = false
        private var configuration: EncodedFrameConfiguration? = null

        private fun handleCodecConfig(data: ByteArray): Boolean {
            val record = runCatching {
                com.picoo.camera.jni.PicooNative.parseCodecConfiguration(request.codec.wireValue, data)
            }.getOrNull() ?: run {
                encoder.fail("Native codec configuration rejected")
                return false
            }
            val source = EncodedFrameConfiguration(request.codec, generationWidth, generationHeight,
                request.framesPerSecond, record)
            configuration = source
            return true
        }

        override fun onInputBufferAvailable(codec: MediaCodec, index: Int) {
            // InputSurface mode: the EGL compositor feeds the encoder.
        }

        override fun onOutputBufferAvailable(
            codec: MediaCodec,
            index: Int,
            info: MediaCodec.BufferInfo,
        ) {
            val active = synchronized(encoder.lifecycle.codecLifecycleLock) {
                generation == encoder.lifecycle.codecGeneration.get() && codec === encoder.mediaCodec
            }
            if (!active || !formatAccepted) {
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
            val maxBytes = if (info.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG != 0) 64 * 1024 else 2 * 1024 * 1024
            if (info.size > maxBytes) {
                runCatching { codec.releaseOutputBuffer(index, false) }
                encoder.fail("Encoded access unit exceeds supported size")
                return
            }
            val data = ByteArray(info.size)
            buffer.position(info.offset)
            buffer.limit(info.offset + info.size)
            buffer.get(data)
            runCatching { codec.releaseOutputBuffer(index, false) }.getOrElse { return }

            if (info.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG != 0) {
                formatAccepted = handleCodecConfig(data)
                return
            }

            val keyFrame = info.flags and MediaCodec.BUFFER_FLAG_KEY_FRAME != 0
            val source = configuration ?: return
            encoder.frameListener.onEncodedFrame(EncodedFrame(
                data = data, isKeyFrame = keyFrame,
                presentationTimeUs = info.presentationTimeUs,
                encodedAtUs = System.nanoTime() / 1_000L,
                streamEpoch = generationEpoch, encoderGeneration = generation,
                configuration = source,
            ))
        }

        override fun onError(codec: MediaCodec, e: MediaCodec.CodecException) {
            val active = synchronized(encoder.lifecycle.codecLifecycleLock) {
                generation == encoder.lifecycle.codecGeneration.get() && codec === encoder.mediaCodec
            }
            if (!active) return
            encoder.fail("MediaCodec error: ${e.diagnosticInfo}")
        }

        override fun onOutputFormatChanged(codec: MediaCodec, format: MediaFormat) {
            val active = synchronized(encoder.lifecycle.codecLifecycleLock) {
                generation == encoder.lifecycle.codecGeneration.get() && codec === encoder.mediaCodec
            }
            if (!active) return
            formatAccepted = false
            NativeVideoEncoder.validateOutput(format, request).getOrElse { error ->
                encoder.fail("Native encoder contract rejected: ${error.message}")
                return
            }
            formatAccepted = false
            val csd0 = format.getByteBuffer("csd-0") ?: run {
                encoder.fail("Native codec configuration missing")
                return
            }
            val csd1 = format.getByteBuffer("csd-1")
            if (csd0.remaining().toLong() + (csd1?.remaining() ?: 0) > 64 * 1024) {
                encoder.fail("Native codec configuration exceeds supported size")
                return
            }
            val copy = ByteArray(csd0.remaining())
            csd0.mark()
            csd0.get(copy)
            csd0.reset()
            if (csd1 != null) {
                val pps = ByteArray(csd1.remaining())
                csd1.mark()
                csd1.get(pps)
                csd1.reset()
                // MediaCodec CSD buffers include Annex B start codes. Normalize
                // both through the same Rust boundary as BUFFER_FLAG_CODEC_CONFIG.
                formatAccepted = handleCodecConfig(copy + pps)
            } else {
                formatAccepted = handleCodecConfig(copy)
            }
        }
    }

    internal fun recordAcceptedFrame(
        byteCount: Int,
        keyFrame: Boolean,
        streamEpoch: Int,
        encoderHeight: Int,
    ) {
        if (keyFrame) {
            encoder.appliedStreamEpoch = streamEpoch
            encoder.appliedEncoderHeight = encoderHeight
        }
        encoder.frameCount += 1
        if (keyFrame) encoder.keyFrameCount += 1
        encoder.bytesSinceLastEstimate += byteCount
        updateBitrateEstimate()
        encoder.stats = EncoderStats(
            encoder.frameCount,
            encoder.keyFrameCount,
            encoder.stats.lastBitrateEstimateKbps,
        )
    }

    private fun updateBitrateEstimate() {
        val now = System.currentTimeMillis()
        val elapsed = now - encoder.lastEstimateAtMs
        if (elapsed >= 1000) {
            val kbps = ((encoder.bytesSinceLastEstimate * 8) / elapsed).toInt()
            encoder.stats = encoder.stats.copy(lastBitrateEstimateKbps = kbps)
            encoder.bytesSinceLastEstimate = 0
            encoder.lastEstimateAtMs = now
        }
    }

    fun requestSyncFrame() {
        val codec = synchronized(encoder.lifecycle.codecLifecycleLock) { encoder.mediaCodec } ?: return
        encoder.codecHandler.post {
            val active = synchronized(encoder.lifecycle.codecLifecycleLock) {
                codec === encoder.mediaCodec
            }
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

    fun applyBitrateIfNeeded() {
        if (encoder.targetBitrateBps == encoder.lastAppliedBitrateBps) return
        val codec = synchronized(encoder.lifecycle.codecLifecycleLock) { encoder.mediaCodec } ?: return
        val requestedBitrate = encoder.targetBitrateBps
        encoder.codecHandler.post {
            val active = synchronized(encoder.lifecycle.codecLifecycleLock) {
                codec === encoder.mediaCodec
            }
            if (active) {
                runCatching {
                    codec.setParameters(
                        android.os.Bundle().apply {
                            putInt(MediaCodec.PARAMETER_KEY_VIDEO_BITRATE, requestedBitrate)
                        },
                    )
                    encoder.lastAppliedBitrateBps = requestedBitrate
                }
            }
        }
    }

    fun release() {
        val detached = detachCodec()
        encoder.codecHandler.post { releaseCodecResources(detached) }
    }

    fun resetCounters() {
        encoder.frameCount = 0
        encoder.keyFrameCount = 0
        encoder.stats = EncoderStats()
    }

    private fun detachCodec(): DetachedCodec = synchronized(encoder.lifecycle.codecLifecycleLock) {
        val generation = encoder.lifecycle.codecGeneration.incrementAndGet()
        DetachedCodec(
            codec = encoder.mediaCodec.also { encoder.mediaCodec = null },
            surface = encoder.codecInputSurface.also { encoder.codecInputSurface = null },
            compositor = encoder.encodingCompositor.also { encoder.encodingCompositor = null },
            nextGeneration = generation,
        )
    }

    private fun releaseCodecResources(detached: DetachedCodec) {
        synchronized(encoder.lifecycle.outputSurfaceLock) {
            // Stop EGL swaps and release Camera2's OES target before invalidating
            // the MediaCodec InputSurface it renders into.
            runCatching { detached.compositor?.close() }
            runCatching { detached.codec?.stop() }
            runCatching { detached.codec?.release() }
            runCatching { detached.surface?.release() }
        }
    }
}
