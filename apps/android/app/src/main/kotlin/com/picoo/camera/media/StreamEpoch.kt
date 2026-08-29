package com.picoo.camera.media

/**
 * stream_epoch bump policy for camera / resolution changes (REQ-PICOO-MEDIA-003).
 *
 * Kept free of Android framework types so JVM unit tests can cover the gate
 * without Camera2 / MediaCodec.
 */
object StreamEpoch {
    /** Initial epoch after encoder construction. */
    const val INITIAL: Int = 1

    /** Monotonic bump used on lens switch or negotiated resolution change. */
    fun bump(current: Int): Int = current + 1

    /** True when a new capture size should advance the epoch and request an IDR. */
    fun shouldBumpForResolution(currentWidth: Int, currentHeight: Int, nextWidth: Int, nextHeight: Int): Boolean =
        currentWidth != nextWidth || currentHeight != nextHeight
}
