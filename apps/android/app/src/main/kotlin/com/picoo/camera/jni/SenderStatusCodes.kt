package com.picoo.camera.jni

/**
 * Sender session status codes / labels aligned with Rust `SenderStatus::as_code` /
 * `as_label` (REQ-PICOO-SESSION-001). Pure JVM — safe for unit tests without JNI.
 */
object SenderStatusCodes {
    const val DISCONNECTED = 0
    const val DISCOVERING = 1
    const val PAIRING = 2
    const val CONNECTING = 3
    const val NEGOTIATING = 4
    const val STREAMING = 5
    const val RECONNECTING = 6
    const val PERMISSION_REQUIRED = 7
    const val NETWORK_UNSTABLE = 8

    fun label(status: Int): String =
        when (status) {
            DISCOVERING -> "Discovering"
            PAIRING -> "Pairing"
            CONNECTING -> "Connecting"
            NEGOTIATING -> "Negotiating"
            STREAMING -> "Streaming"
            RECONNECTING -> "Reconnecting"
            PERMISSION_REQUIRED -> "Permission Required"
            NETWORK_UNSTABLE -> "Network Unstable"
            else -> "Disconnected"
        }
}
