package com.picoo.camera.media

import com.picoo.camera.jni.PicooNative

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

/** Live streaming still owns a Core epoch; the disconnected viewfinder does not. */
internal fun cameraSwitchUsesLiveReconfiguration(status: Int): Boolean =
    status == PicooNative.STATUS_STREAMING || status == PicooNative.STATUS_NETWORK_UNSTABLE
