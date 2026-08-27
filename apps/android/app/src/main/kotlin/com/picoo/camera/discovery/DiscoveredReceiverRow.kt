package com.picoo.camera.discovery

import com.picoo.camera.jni.PicooNative

/**
 * Devices-list row formatting — REQ-PICOO-DISCOVERY-005 / REQ-PICOO-UI-003.
 *
 * Ready = discoverable for new pairing; Paired = local trusted store match
 * (or TXT `paired_only`). Platform is not in the TXT whitelist for A→W V1.
 */
object DiscoveredReceiverRow {
    fun readinessLabel(
        pairingState: String?,
        locallyTrusted: Boolean,
    ): String =
        when {
            locallyTrusted -> "Paired"
            pairingState.equals("paired_only", ignoreCase = true) -> "Paired"
            else -> "Ready"
        }

    fun format(
        receiver: PicooNative.DiscoveredReceiver,
        locallyTrusted: Boolean,
    ): String {
        val readiness = readinessLabel(receiver.pairingState, locallyTrusted)
        return "${receiver.displayName} · $readiness (${receiver.host}:${receiver.quicPort})"
    }
}
