package com.picoo.camera.media

/** One native encoder generation's validated standard configuration record. */
data class EncodedFrameConfiguration(
    val codec: NativeVideoCodec,
    val width: Int,
    val height: Int,
    val framesPerSecond: Int,
    val record: ByteArray,
)

/** Detached compressed bytes and the original generation's configuration. */
data class EncodedFrame(
    val data: ByteArray,
    val isKeyFrame: Boolean,
    val presentationTimeUs: Long,
    val encodedAtUs: Long,
    val streamEpoch: Int,
    val encoderGeneration: Long,
    val configuration: EncodedFrameConfiguration,
)

fun interface EncodedFrameListener {
    fun onEncodedFrame(frame: EncodedFrame)
    companion object { val NOOP = EncodedFrameListener { } }
}

data class EncoderStats(
    val frameCount: Int = 0,
    val keyFrameCount: Int = 0,
    val lastBitrateEstimateKbps: Int = 0,
)
