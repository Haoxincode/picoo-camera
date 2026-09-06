package com.picoo.camera.media

import android.graphics.Color
import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaCodecList
import android.media.MediaFormat
import android.util.Log
import android.util.Size
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** REQ-PICOO-NEXT-003/004/009: real hardware contract, not a throughput acceptance. */
class NativeCodecContractTest {
    @Test
    fun avcAndHevcHardwareSurfaceConfigurationsProduceAccessUnits() {
        for (mime in listOf(MediaFormat.MIMETYPE_VIDEO_AVC, MediaFormat.MIMETYPE_VIDEO_HEVC)) {
            for (size in listOf(Size(1280, 720), Size(1920, 1080))) {
                for (fps in listOf(30, 60)) verify(mime, size, fps)
            }
        }
    }

    private fun verify(mime: String, size: Size, fps: Int) {
        val profile = if (mime == MediaFormat.MIMETYPE_VIDEO_AVC) {
            MediaCodecInfo.CodecProfileLevel.AVCProfileHigh
        } else {
            MediaCodecInfo.CodecProfileLevel.HEVCProfileMain
        }
        val candidate = MediaCodecList(MediaCodecList.ALL_CODECS).codecInfos.firstOrNull { info ->
            info.isEncoder && info.isHardwareAccelerated && !info.isAlias &&
                info.supportedTypes.any { it.equals(mime, ignoreCase = true) } &&
                info.getCapabilitiesForType(mime).let { caps ->
                    caps.colorFormats.contains(MediaCodecInfo.CodecCapabilities.COLOR_FormatSurface) &&
                        caps.profileLevels.any { it.profile == profile } &&
                        caps.videoCapabilities.areSizeAndRateSupported(size.width, size.height, fps.toDouble())
                }
        } ?: error("No hardware $mime $size @$fps profile=$profile")
        val codec = MediaCodec.createByCodecName(candidate.name)
        var surface: android.view.Surface? = null
        var compositor: CameraEncodingCompositor? = null
        val errors = mutableListOf<String>()
        try {
            val format = MediaFormat.createVideoFormat(mime, size.width, size.height).apply {
                setInteger(MediaFormat.KEY_COLOR_FORMAT, MediaCodecInfo.CodecCapabilities.COLOR_FormatSurface)
                setInteger(MediaFormat.KEY_PROFILE, profile)
                setInteger(MediaFormat.KEY_BIT_RATE, if (mime == MediaFormat.MIMETYPE_VIDEO_HEVC) 16_000_000 else 24_000_000)
                setInteger(MediaFormat.KEY_FRAME_RATE, fps)
                setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, 1)
                setInteger(MediaFormat.KEY_MAX_B_FRAMES, 0)
                setInteger(MediaFormat.KEY_COLOR_STANDARD, MediaFormat.COLOR_STANDARD_BT709)
                setInteger(MediaFormat.KEY_COLOR_RANGE, MediaFormat.COLOR_RANGE_LIMITED)
                setInteger(MediaFormat.KEY_COLOR_TRANSFER, MediaFormat.COLOR_TRANSFER_SDR_VIDEO)
            }
            codec.configure(format, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
            surface = codec.createInputSurface()
            compositor = CameraEncodingCompositor.create(
                encoderSurface = surface,
                cameraBufferSize = size,
                outputSize = size,
                initialRotationDegrees = 0,
                onError = { synchronized(errors) { errors += it } },
            ).getOrThrow()
            codec.start()
            assertTrue(codec.codecInfo.isHardwareAccelerated)
            val info = MediaCodec.BufferInfo()
            val timestamps = mutableListOf<Long>()
            var actual: MediaFormat? = null
            repeat(3) { frame ->
                // Diagnostic fixture only: GPU compositor then MediaCodec InputSurface.
                val canvas = compositor.cameraInputSurface.lockCanvas(null)
                try { canvas.drawColor(Color.rgb(40 + frame * 30, 120, 180)) }
                finally { compositor.cameraInputSurface.unlockCanvasAndPost(canvas) }
                val deadline = System.nanoTime() + 5_000_000_000L
                while (timestamps.size <= frame && System.nanoTime() < deadline) {
                    val ix = codec.dequeueOutputBuffer(info, 100_000)
                    if (ix == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED) actual = codec.outputFormat
                    if (ix >= 0) {
                        try {
                            if (info.size > 0 && info.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG == 0) {
                                timestamps += info.presentationTimeUs
                            }
                        } finally { codec.releaseOutputBuffer(ix, false) }
                    }
                }
                assertEquals("$mime $size @$fps output deadline", frame + 1, timestamps.size)
            }
            assertTrue("PTS must identify distinct ordered images", timestamps.zipWithNext().all { (a, b) -> b > a })
            assertTrue("compositor errors: $errors", synchronized(errors) { errors.isEmpty() })
            val output = actual ?: error("No actual output format")
            assertEquals(mime, output.getString(MediaFormat.KEY_MIME))
            assertEquals(size.width, output.getInteger(MediaFormat.KEY_WIDTH))
            assertEquals(size.height, output.getInteger(MediaFormat.KEY_HEIGHT))
            assertTrue("Missing codec configuration", output.containsKey("csd-0"))
            Log.i("PicooNativeProbe", "codec=${candidate.name}; mime=$mime; size=$size; requested_fps=$fps; " +
                "hardware=true; unique_aus=${timestamps.size}; actual=$output; steady_state_fps=not_measured")
        } finally {
            compositor?.close()
            runCatching { codec.stop() }
            codec.release()
            surface?.release()
        }
    }
}
