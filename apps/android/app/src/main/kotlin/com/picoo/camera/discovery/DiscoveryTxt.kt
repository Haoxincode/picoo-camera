package com.picoo.camera.discovery

import com.picoo.camera.jni.PicooNative

/**
 * TXT attribute parsing for `_picoocam._udp` (REQ-PICOO-DISCOVERY-005).
 *
 * Android owns NSD transport; Rust Core owns the TXT schema and validation.
 */
object DiscoveryTxt {
    const val SERVICE_TYPE: String = "_picoocam._udp"

    data class Parsed(
        val receiverId: String,
        val displayName: String,
        val protocolVersion: String?,
        val quicPort: Int,
        val pairingState: String?,
        val fingerprintPrefix: String?,
    )

    fun parseAttributes(raw: Map<String, ByteArray?>): Parsed? {
        if (raw.isEmpty() || raw.values.any { it == null }) return null
        val entries = raw.entries.toList()
        val fields = PicooNative.parseDiscoveryTxt(
            keys = entries.map { it.key }.toTypedArray(),
            values = entries.map { requireNotNull(it.value) }.toTypedArray(),
        ) ?: return null
        if (fields.size < 6) return null
        return Parsed(
            receiverId = fields[0],
            displayName = fields[1],
            protocolVersion = fields[2],
            quicPort = fields[3].toInt(),
            pairingState = fields[4],
            fingerprintPrefix = fields[5],
        )
    }
}
