package com.picoo.camera.media

import android.hardware.camera2.CameraDevice
import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.util.Size
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
        val encodeSize = encoder.profile.resolution
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
    ) = object : MediaCodec.Callback() {
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
                encoder.appliedStreamEpoch = generationEpoch
                encoder.appliedEncoderHeight = generationHeight
            }
            encoder.frameCount += 1
            if (keyFrame) encoder.keyFrameCount += 1
            encoder.bytesSinceLastEstimate += info.size
            updateBitrateEstimate()
            encoder.stats = EncoderStats(
                encoder.frameCount,
                encoder.keyFrameCount,
                encoder.stats.lastBitrateEstimateKbps,
            )
            encoder.frameListener.onEncodedFrame(
                data,
                keyFrame,
                info.presentationTimeUs,
                generationEpoch,
                generation,
                generationWidth,
                generationHeight,
            )
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
        encoder.lastSps = sps
        encoder.lastPps = pps
        encoder.parameterSetsListener.onParameterSets(sps, pps)
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

    private fun createEncoder(size: Size): MediaCodec? {
        encoder.lastAppliedBitrateBps = encoder.targetBitrateBps
        val codec = runCatching {
            MediaCodec.createEncoderByType(MediaFormat.MIMETYPE_VIDEO_AVC)
        }.getOrNull() ?: return null
        val bitrateMode = runCatching {
            val capabilities = codec.codecInfo.getCapabilitiesForType(MediaFormat.MIMETYPE_VIDEO_AVC)
            if (capabilities.encoderCapabilities.isBitrateModeSupported(
                    MediaCodecInfo.EncoderCapabilities.BITRATE_MODE_CBR,
                )
            ) {
                MediaCodecInfo.EncoderCapabilities.BITRATE_MODE_CBR
            } else {
                MediaCodecInfo.EncoderCapabilities.BITRATE_MODE_VBR
            }
        }.getOrDefault(MediaCodecInfo.EncoderCapabilities.BITRATE_MODE_VBR)
        val format = MediaFormat.createVideoFormat(
            MediaFormat.MIMETYPE_VIDEO_AVC,
            size.width,
            size.height,
        ).apply {
            setInteger(MediaFormat.KEY_COLOR_FORMAT, MediaCodecInfo.CodecCapabilities.COLOR_FormatSurface)
            setInteger(MediaFormat.KEY_BIT_RATE, encoder.targetBitrateBps)
            setInteger(MediaFormat.KEY_FRAME_RATE, encoder.profile.targetFps)
            setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, 2)
            setInteger(MediaFormat.KEY_BITRATE_MODE, bitrateMode)
            setInteger(MediaFormat.KEY_COLOR_STANDARD, MediaFormat.COLOR_STANDARD_BT709)
            setInteger(MediaFormat.KEY_COLOR_RANGE, MediaFormat.COLOR_RANGE_LIMITED)
            setInteger(MediaFormat.KEY_COLOR_TRANSFER, MediaFormat.COLOR_TRANSFER_SDR_VIDEO)
            // Real-time camera transport must not inherit the vendor encoder's
            // implementation-defined look-ahead (four frames on Xiaomi 15).
            // Android defines this value in frames and requires the negotiated
            // output format to be inspected to confirm what was accepted.
            setInteger(MediaFormat.KEY_LATENCY, 0)
            setInteger(MediaFormat.KEY_MAX_B_FRAMES, 0)
            setInteger(MediaFormat.KEY_OUTPUT_REORDER_DEPTH, 0)
            setInteger(MediaFormat.KEY_PRIORITY, 0)
            setFloat(MediaFormat.KEY_OPERATING_RATE, encoder.profile.targetFps.toFloat())
        }
        return runCatching {
            codec.configure(format, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
            codec
        }.getOrElse {
            runCatching { codec.release() }
            null
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
