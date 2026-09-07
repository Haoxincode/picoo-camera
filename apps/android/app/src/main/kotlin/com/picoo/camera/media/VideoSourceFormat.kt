package com.picoo.camera.media

/** Complete product request; local capture and actual output still require admission (MEDIA-054). */
data class VideoSourceFormat(
    val codec: NativeVideoCodec,
    val resolution: StreamResolution,
    val framesPerSecond: Int,
) {
    init { require(framesPerSecond == 30 || framesPerSecond == 60) }

    val label: String
        get() = "${if (codec == NativeVideoCodec.Avc) "H.264" else "HEVC"} · ${resolution.label} · $framesPerSecond fps"

    fun matches(profile: CaptureProfile): Boolean =
        codec == profile.codec && framesPerSecond == profile.targetFps &&
            resolution.width == profile.resolution.width && resolution.height == profile.resolution.height

    companion object {
        val Default = VideoSourceFormat(NativeVideoCodec.Avc, StreamResolution.P1080, 60)
        val ProductFormats: List<VideoSourceFormat> = NativeVideoCodec.entries.flatMap { codec ->
            StreamResolution.entries.flatMap { resolution ->
                listOf(60, 30).map { fps -> VideoSourceFormat(codec, resolution, fps) }
            }
        }

        fun fromWire(codec: Int, height: Int, framesPerSecond: Int): VideoSourceFormat? {
            val nativeCodec = NativeVideoCodec.entries.firstOrNull { it.wireValue == codec } ?: return null
            val resolution = StreamResolution.fromHeight(height) ?: return null
            if (framesPerSecond != 30 && framesPerSecond != 60) return null
            return VideoSourceFormat(nativeCodec, resolution, framesPerSecond)
        }
    }
}
