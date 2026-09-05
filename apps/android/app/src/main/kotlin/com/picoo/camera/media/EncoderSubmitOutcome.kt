package com.picoo.camera.media

/** Typed Kotlin boundary for the JNI access-unit submission result. */
internal sealed interface EncoderSubmitOutcome {
    data class Success(
        val encoderAccepted: Boolean,
        val streamConfigured: Boolean,
        val keyframeRequested: Boolean,
    ) : EncoderSubmitOutcome

    data class Failure(val nativeCode: Int) : EncoderSubmitOutcome

    companion object {
        private const val ENCODER_ACCEPTED = 1
        private const val STREAM_CONFIGURED = 1 shl 1
        private const val KEYFRAME_REQUESTED = 1 shl 2

        fun fromNative(result: Int): EncoderSubmitOutcome {
            if (result < 0) return Failure(result)
            return Success(
                encoderAccepted = result and ENCODER_ACCEPTED != 0,
                streamConfigured = result and STREAM_CONFIGURED != 0,
                keyframeRequested = result and KEYFRAME_REQUESTED != 0,
            )
        }
    }
}
