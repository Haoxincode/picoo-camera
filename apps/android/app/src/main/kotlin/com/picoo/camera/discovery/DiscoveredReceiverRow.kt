package com.picoo.camera.discovery

import com.picoo.camera.jni.PicooNative

/**
 * Devices-list row formatting — REQ-PICOO-DISCOVERY-005 / REQ-PICOO-UI-003 / PRD §17.1.
 *
 * A→W V1 hardcodes platform `Windows` (not in TXT whitelist).
 * Ready/Paired follows local trusted store; TXT `paired_only` is a secondary signal.
 */
object DiscoveredReceiverRow {
    const val PLATFORM_WINDOWS: String = "Windows"

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
        return "${receiver.displayName} · $PLATFORM_WINDOWS · $readiness"
    }
}
