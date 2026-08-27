package com.picoo.camera.discovery

import com.picoo.camera.jni.PicooNative

/**
 * Prefer auto-connecting to already-paired receivers discovered via NSD (PUC-002).
 * Tap-to-connect remains available for unpaired devices.
 */
object PairedAutoConnect {
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
