package com.picoo.camera.media

import androidx.annotation.Keep

/** REQ-PICOO-NEXT-026: JNI returns separate disposition and side-effect facts. */
sealed interface EncoderSubmitOutcome {
    @Keep
    data class Accepted(
        val streamConfigured: Boolean,
        val keyframeRequested: Boolean,
    ) : EncoderSubmitOutcome

    @Keep
    data class Rejected(val keyframeRequested: Boolean) : EncoderSubmitOutcome

    @Keep
    data class Error(val nativeCode: Int) : EncoderSubmitOutcome
}
