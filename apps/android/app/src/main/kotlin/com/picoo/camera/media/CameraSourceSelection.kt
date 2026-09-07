package com.picoo.camera.media

/** MEDIA-060: an explicit lens switch chooses that lens's complete admitted ceiling. */
internal object CameraSourceSelection {
    fun initial(candidates: List<VideoSourceFormat>, preferred: VideoSourceFormat): VideoSourceFormat? =
        preferred.takeIf { it in candidates } ?: select(candidates, preferred.codec)

    fun select(candidates: List<VideoSourceFormat>, preferredCodec: NativeVideoCodec): VideoSourceFormat? {
        val sameCodec = candidates.filter { it.codec == preferredCodec }
        return sameCodec.ifEmpty { candidates }.maxWithOrNull(
            compareBy<VideoSourceFormat> { it.resolution.height }
                .thenBy { it.framesPerSecond }
                .thenBy { -it.codec.wireValue },
        )
    }
}
