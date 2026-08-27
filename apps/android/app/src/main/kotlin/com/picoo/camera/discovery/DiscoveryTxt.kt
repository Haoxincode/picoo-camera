package com.picoo.camera.discovery

/**
 * TXT attribute parsing for `_picoocam._udp` (REQ-PICOO-DISCOVERY-005).
 *
 * Keys must stay aligned with `picoo_discovery::ALLOWED_TXT_KEYS`.
 */
object DiscoveryTxt {
    const val SERVICE_TYPE: String = "_picoocam._udp"

    val ALLOWED_KEYS: Set<String> =
        setOf(
            "receiver_id",
            "display_name",
            "protocol_version",
            "quic_port",
            "pairing_state",
            "public_key_fingerprint_prefix",
        )

    data class Parsed(
        val receiverId: String,
        val displayName: String,
        val protocolVersion: String?,
        val quicPort: Int,
        val pairingState: String?,
        val fingerprintPrefix: String?,
    )

    fun parseAttributes(raw: Map<String, ByteArray?>): Parsed? {
        val values = LinkedHashMap<String, String>()
        for ((key, bytes) in raw) {
            if (key !in ALLOWED_KEYS) continue
            val text = bytes?.toString(Charsets.UTF_8)?.trim().orEmpty()
            if (text.isNotEmpty()) {
                values[key] = text
            }
        }
        val receiverId = values["receiver_id"] ?: return null
        val quicPort = values["quic_port"]?.toIntOrNull()?.takeIf { it in 1..65535 } ?: return null
        return Parsed(
            receiverId = receiverId,
            displayName = values["display_name"] ?: receiverId,
            protocolVersion = values["protocol_version"],
            quicPort = quicPort,
            pairingState = values["pairing_state"],
            fingerprintPrefix = values["public_key_fingerprint_prefix"],
        )
    }
}
