package com.picoo.camera.discovery

import com.picoo.camera.jni.PicooNative

/**
 * Prefer auto-connecting to already-paired receivers discovered via NSD (PUC-002).
 * Tap-to-connect remains available for unpaired devices.
 */
object PairedAutoConnect {
    /**
     * A Receiver StopStream moves an authenticated live session directly to Disconnected.
     * Network loss instead enters Reconnecting inside Rust, so this transition is the host-side
     * signal that automatic discovery must not immediately undo the remote user's disconnect.
     */
    fun shouldSuppressAfterRemoteStop(previousStatus: Int, currentStatus: Int): Boolean {
        if (currentStatus != PicooNative.STATUS_DISCONNECTED) return false
        return when (previousStatus) {
            PicooNative.STATUS_STREAMING,
            PicooNative.STATUS_NETWORK_UNSTABLE,
            PicooNative.STATUS_PERMISSION_REQUIRED,
            -> true
            else -> false
        }
    }

    fun pick(
        discovered: List<PicooNative.DiscoveredReceiver>,
        pairedReceiverIds: Set<String>,
        sessionBusy: Boolean,
        alreadyAttemptedIds: Set<String>,
    ): PicooNative.DiscoveredReceiver? {
        if (sessionBusy || pairedReceiverIds.isEmpty() || discovered.isEmpty()) {
            return null
        }
        return discovered.firstOrNull { receiver ->
            pairedReceiverIds.contains(receiver.receiverId) &&
                !alreadyAttemptedIds.contains(receiver.receiverId)
        }
    }
}
