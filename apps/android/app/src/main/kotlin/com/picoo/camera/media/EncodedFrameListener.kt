package com.picoo.camera.media

/**
 * Callback for encoded H.264 access units from MediaCodec (REQ-PICOO-MEDIA-001).
 *
 * Raw YUV must not cross JNI; only encoded AU bytes are forwarded toward Rust/QUIC.
 */
fun interface EncodedFrameListener {
    fun onEncodedFrame(
        data: ByteArray,
        isKeyFrame: Boolean,
        presentationTimeUs: Long,
        streamEpoch: Int,
    )

    companion object {
        val NOOP = EncodedFrameListener { _, _, _, _ -> }
    }
}

data class EncoderStats(
    val frameCount: Int = 0,
    val keyFrameCount: Int = 0,
    val lastBitrateEstimateKbps: Int = 0,
)
