package com.picoo.camera.media

import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaCodecList
import android.media.MediaFormat
import android.util.Size

enum class NativeVideoCodec(val wireValue: Int, val mime: String, val profile: Int) {
    Avc(1, MediaFormat.MIMETYPE_VIDEO_AVC, MediaCodecInfo.CodecProfileLevel.AVCProfileHigh),
    Hevc(2, MediaFormat.MIMETYPE_VIDEO_HEVC, MediaCodecInfo.CodecProfileLevel.HEVCProfileMain),
}

/** Immutable request owned by one encoder generation. */
internal data class NativeEncoderFormat(
    val codec: NativeVideoCodec,
    val size: Size,
    val framesPerSecond: Int,
    val bitrateBps: Int,
)

/** Official MediaCodec Surface adapter — REQ-PICOO-MEDIA-026. */
internal object NativeVideoEncoder {
    fun validateOutput(format: MediaFormat, request: NativeEncoderFormat): Result<Unit> = runCatching {
        check(format.getString(MediaFormat.KEY_MIME) == request.codec.mime) { "Encoder changed codec" }
        val profile = format.getInteger(MediaFormat.KEY_PROFILE)
        val profileAccepted = profile == request.codec.profile ||
            (request.codec == NativeVideoCodec.Avc &&
                profile == MediaCodecInfo.CodecProfileLevel.AVCProfileConstrainedHigh)
        check(profileAccepted) { "Encoder changed profile" }
        check(format.getInteger(MediaFormat.KEY_WIDTH) == request.size.width &&
            format.getInteger(MediaFormat.KEY_HEIGHT) == request.size.height) { "Encoder changed dimensions" }
        check(format.getInteger(MediaFormat.KEY_FRAME_RATE) == request.framesPerSecond) { "Encoder changed frame rate" }
        check(format.getInteger(MediaFormat.KEY_COLOR_STANDARD) == MediaFormat.COLOR_STANDARD_BT709 &&
            format.getInteger(MediaFormat.KEY_COLOR_RANGE) == MediaFormat.COLOR_RANGE_LIMITED &&
            format.getInteger(MediaFormat.KEY_COLOR_TRANSFER) == MediaFormat.COLOR_TRANSFER_SDR_VIDEO) {
            "Encoder changed SDR color description"
        }
        check(format.containsKey("csd-0")) { "Encoder configuration is missing" }
    }

    private fun candidate(request: NativeEncoderFormat): MediaCodecInfo {
        require(request.framesPerSecond in listOf(30, 60)) { "Unsupported source frame rate" }
        require(request.size in listOf(Size(1280, 720), Size(1920, 1080))) {
            "Unsupported source dimensions"
        }
        require(request.bitrateBps > 0) { "Invalid encoder bitrate" }
        return MediaCodecList(MediaCodecList.ALL_CODECS).codecInfos.firstOrNull { info ->
            info.isEncoder && info.isHardwareAccelerated && !info.isAlias &&
                info.supportedTypes.any { it.equals(request.codec.mime, ignoreCase = true) } &&
                runCatching {
                    info.getCapabilitiesForType(request.codec.mime).let { caps ->
                        (caps.encoderCapabilities.isBitrateModeSupported(MediaCodecInfo.EncoderCapabilities.BITRATE_MODE_CBR) ||
                            caps.encoderCapabilities.isBitrateModeSupported(MediaCodecInfo.EncoderCapabilities.BITRATE_MODE_VBR)) &&
                            caps.colorFormats.contains(MediaCodecInfo.CodecCapabilities.COLOR_FormatSurface) &&
                            caps.profileLevels.any { it.profile == request.codec.profile } &&
                            caps.videoCapabilities.bitrateRange.contains(request.bitrateBps) &&
                            caps.videoCapabilities.areSizeAndRateSupported(
                                request.size.width, request.size.height, request.framesPerSecond.toDouble(),
                            )
                    }
                }.getOrDefault(false)
        } ?: error("No hardware encoder for $request")
    }

    /** Preparation only; configure/output records still require admission (MEDIA-056). */
    fun supports(request: NativeEncoderFormat): Boolean = runCatching { candidate(request) }.isSuccess

    fun create(request: NativeEncoderFormat): Result<MediaCodec> = runCatching {
        val candidate = candidate(request)
        val codec = MediaCodec.createByCodecName(candidate.name)
        try {
            check(codec.codecInfo.isHardwareAccelerated) { "Selected encoder is not hardware accelerated" }
            val caps = candidate.getCapabilitiesForType(request.codec.mime)
            val bitrateMode = if (caps.encoderCapabilities.isBitrateModeSupported(
                    MediaCodecInfo.EncoderCapabilities.BITRATE_MODE_CBR,
                )
            ) MediaCodecInfo.EncoderCapabilities.BITRATE_MODE_CBR
            else MediaCodecInfo.EncoderCapabilities.BITRATE_MODE_VBR
            val format = MediaFormat.createVideoFormat(
                request.codec.mime, request.size.width, request.size.height,
            ).apply {
                setInteger(MediaFormat.KEY_COLOR_FORMAT, MediaCodecInfo.CodecCapabilities.COLOR_FormatSurface)
                setInteger(MediaFormat.KEY_PROFILE, request.codec.profile)
                setInteger(MediaFormat.KEY_BIT_RATE, request.bitrateBps)
                setInteger(MediaFormat.KEY_BITRATE_MODE, bitrateMode)
                setInteger(MediaFormat.KEY_FRAME_RATE, request.framesPerSecond)
                setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, 2)
                setInteger(MediaFormat.KEY_MAX_B_FRAMES, 0)
                setInteger(MediaFormat.KEY_OUTPUT_REORDER_DEPTH, 0)
                setInteger(MediaFormat.KEY_LATENCY, 0)
                setInteger(MediaFormat.KEY_PRIORITY, 0)
                setFloat(MediaFormat.KEY_OPERATING_RATE, request.framesPerSecond.toFloat())
                setInteger(MediaFormat.KEY_COLOR_STANDARD, MediaFormat.COLOR_STANDARD_BT709)
                setInteger(MediaFormat.KEY_COLOR_RANGE, MediaFormat.COLOR_RANGE_LIMITED)
                setInteger(MediaFormat.KEY_COLOR_TRANSFER, MediaFormat.COLOR_TRANSFER_SDR_VIDEO)
            }
            codec.configure(format, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
            codec
        } catch (error: Throwable) {
            runCatching { codec.release() }
            throw error
        }
    }
}
