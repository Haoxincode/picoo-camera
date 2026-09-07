package com.picoo.camera.media

/** MEDIA-060: an explicit lens switch chooses that lens's complete admitted ceiling. */
internal object CameraSourceSelection {
    fun select(candidates: List<VideoSourceFormat>, preferredCodec: NativeVideoCodec): VideoSourceFormat? {
        val sameCodec = candidates.filter { it.codec == preferredCodec }
        return sameCodec.ifEmpty { candidates }.maxWithOrNull(
            compareBy<VideoSourceFormat> { it.resolution.height }
                .thenBy { it.framesPerSecond }
                .thenBy { -it.codec.wireValue },
        )
    }
}
