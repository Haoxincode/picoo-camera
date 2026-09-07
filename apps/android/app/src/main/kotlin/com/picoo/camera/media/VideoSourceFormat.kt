package com.picoo.camera.media

/** Complete product request; local capture and actual output still require admission (MEDIA-054). */
data class VideoSourceFormat(
    val codec: NativeVideoCodec,
    val resolution: StreamResolution,
    val framesPerSecond: Int,
) {
    init { require(framesPerSecond == 30 || framesPerSecond == 60) }

    fun matches(profile: CaptureProfile): Boolean =
        codec == profile.codec && framesPerSecond == profile.targetFps &&
            resolution.width == profile.resolution.width && resolution.height == profile.resolution.height

    companion object {
        fun fromWire(codec: Int, height: Int, framesPerSecond: Int): VideoSourceFormat? {
            val nativeCodec = NativeVideoCodec.entries.firstOrNull { it.wireValue == codec } ?: return null
            val resolution = StreamResolution.fromHeight(height) ?: return null
            if (framesPerSecond != 30 && framesPerSecond != 60) return null
            return VideoSourceFormat(nativeCodec, resolution, framesPerSecond)
        }
    }
}
