package com.picoo.camera.discovery

import com.picoo.camera.jni.PicooNative

/**
 * Devices-list row formatting — REQ-PICOO-DISCOVERY-005 / REQ-PICOO-UI-003 / PRD §17.1.
 *
 * Platform comes from the Rust-validated TXT whitelist.
 * Ready/Paired follows local trusted store; TXT `paired_only` is a secondary signal.
 */
object DiscoveredReceiverRow {
    fun platformLabel(platform: String): String = when (platform) {
        "windows" -> "Windows"
        "macos" -> "macOS"
        else -> platform
    }

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
        return "${receiver.displayName} · ${platformLabel(receiver.platform)} · $readiness"
    }
}
