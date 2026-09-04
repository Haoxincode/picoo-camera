package com.picoo.camera.discovery

import com.picoo.camera.jni.PicooNative

/**
 * TXT attribute parsing for `_picoocam._udp` (REQ-PICOO-DISCOVERY-005).
 *
 * Android owns NSD transport; Rust Core owns the TXT schema and validation.
 */
object DiscoveryTxt {
    const val SERVICE_TYPE: String = "_picoocam._udp"

    internal data class JniAttributes(
        val keys: Array<String>,
        val values: Array<ByteArray>,
    )

    data class Parsed(
        val receiverId: String,
        val displayName: String,
        val quicPort: Int,
        val pairingState: String?,
        val fingerprintPrefix: String?,
        val platform: String,
    )

    fun parseAttributes(raw: Map<String, ByteArray?>): Parsed? {
        val attributes = copyForJni(raw) ?: return null
        val fields = PicooNative.parseDiscoveryTxt(
            keys = attributes.keys,
            values = attributes.values,
        ) ?: return null
        if (fields.size < 6) return null
        return Parsed(
            receiverId = fields[0],
            displayName = fields[1],
            quicPort = fields[2].toInt(),
            pairingState = fields[3],
            fingerprintPrefix = fields[4],
            platform = fields[5],
        )
    }

    /**
     * Copy Android's platform-owned NSD attribute map without using Collection.toArray().
     *
     * Some Android ArrayMap-backed unmodifiable entry sets, including Xiaomi 15's HyperOS
     * implementation, support iteration but throw UnsupportedOperationException from toArray().
     * Kotlin's Collection.toList() delegates to that method and would terminate ConnectivityThread.
     */
    internal fun copyForJni(raw: Map<String, ByteArray?>): JniAttributes? {
        if (raw.isEmpty()) return null
        val keys = ArrayList<String>(raw.size)
        val values = ArrayList<ByteArray>(raw.size)
        val iterator = raw.entries.iterator()
        while (iterator.hasNext()) {
            val entry = iterator.next()
            val value = entry.value ?: return null
            keys.add(entry.key)
            values.add(value)
        }
        if (keys.isEmpty()) return null
        return JniAttributes(
            keys = keys.toTypedArray(),
            values = values.toTypedArray(),
        )
    }
}
