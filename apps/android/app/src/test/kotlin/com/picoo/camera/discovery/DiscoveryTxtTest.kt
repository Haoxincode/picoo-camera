package com.picoo.camera.discovery

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** REQ-PICOO-DISCOVERY-005 TXT whitelist parsing. */
class DiscoveryTxtTest {
    @Test
    fun parsesWhitelistedAttributes() {
        val parsed =
            DiscoveryTxt.parseAttributes(
                mapOf(
                    "receiver_id" to "windows-receiver".toByteArray(),
                    "display_name" to "Office PC".toByteArray(),
                    "protocol_version" to "picoocam/1".toByteArray(),
                    "quic_port" to "4747".toByteArray(),
                    "pairing_state" to "open".toByteArray(),
                    "public_key_fingerprint_prefix" to "abcd1234".toByteArray(),
                    "evil_extra" to "drop-me".toByteArray(),
                ),
            )
        requireNotNull(parsed)
        assertEquals("windows-receiver", parsed.receiverId)
        assertEquals("Office PC", parsed.displayName)
        assertEquals(4747, parsed.quicPort)
        assertEquals("picoocam/1", parsed.protocolVersion)
        assertEquals("open", parsed.pairingState)
        assertEquals("abcd1234", parsed.fingerprintPrefix)
    }

    @Test
    fun rejectsMissingReceiverIdOrPort() {
        assertNull(
            DiscoveryTxt.parseAttributes(
                mapOf("display_name" to "x".toByteArray(), "quic_port" to "1".toByteArray()),
            ),
        )
        assertNull(
            DiscoveryTxt.parseAttributes(
                mapOf("receiver_id" to "x".toByteArray(), "quic_port" to "bad".toByteArray()),
            ),
        )
    }
}
