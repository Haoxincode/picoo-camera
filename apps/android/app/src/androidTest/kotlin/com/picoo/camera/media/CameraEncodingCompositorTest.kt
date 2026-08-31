package com.picoo.camera.media

import android.graphics.Color
import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaFormat
import android.util.Size
import org.junit.Assert.assertTrue
import org.junit.Test

class CameraEncodingCompositorTest {
    @Test
    fun oesFrameReachesFixedLandscapeHardwareEncoderSurface() {
        val outputSize = Size(640, 360)
        val codec = MediaCodec.createEncoderByType(MediaFormat.MIMETYPE_VIDEO_AVC)
        val format = MediaFormat.createVideoFormat(
            MediaFormat.MIMETYPE_VIDEO_AVC,
            outputSize.width,
            outputSize.height,
        ).apply {
            setInteger(
                MediaFormat.KEY_COLOR_FORMAT,
                MediaCodecInfo.CodecCapabilities.COLOR_FormatSurface,
            )
            setInteger(MediaFormat.KEY_BIT_RATE, 1_000_000)
            setInteger(MediaFormat.KEY_FRAME_RATE, 30)
            setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, 1)
        }
        codec.configure(format, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
        val encoderSurface = codec.createInputSurface()
        val errors = mutableListOf<String>()
        val compositor = CameraEncodingCompositor.create(
            encoderSurface = encoderSurface,
            cameraBufferSize = Size(360, 640),
            outputSize = outputSize,
            initialRotationDegrees = 90,
            onError = { synchronized(errors) { errors += it } },
        ).getOrThrow()

        try {
            codec.start()
            val canvas = compositor.cameraInputSurface.lockCanvas(null)
            try {
                canvas.drawColor(Color.rgb(30, 120, 210))
            } finally {
                compositor.cameraInputSurface.unlockCanvasAndPost(canvas)
            }

            val info = MediaCodec.BufferInfo()
            val deadline = System.nanoTime() + ENCODE_TIMEOUT_NANOS
            var encodedFrameObserved = false
            while (System.nanoTime() < deadline && !encodedFrameObserved) {
                val index = codec.dequeueOutputBuffer(info, DEQUEUE_TIMEOUT_US)
                if (index >= 0) {
                    encodedFrameObserved = info.size > 0 &&
                        info.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG == 0
                    codec.releaseOutputBuffer(index, false)
                }
            }

            assertTrue("compositor errors: $errors", errors.isEmpty())
            assertTrue("GPU-composited frame did not reach MediaCodec", encodedFrameObserved)
        } finally {
            compositor.close()
            runCatching { codec.stop() }
            codec.release()
            encoderSurface.release()
        }
    }

    private companion object {
        const val DEQUEUE_TIMEOUT_US = 100_000L
        const val ENCODE_TIMEOUT_NANOS = 5_000_000_000L
    }
}
