package com.picoo.camera.jni

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * Keep diagnostics / UI status strings aligned with Rust `SenderStatus::as_label`
 * (REQ-PICOO-SESSION-001 / PRIVACY-003).
 */
class SenderStatusLabelTest {
    @Test
    fun statusLabelsMatchRustAsLabel() {
        assertEquals("Disconnected", SenderStatusCodes.label(SenderStatusCodes.DISCONNECTED))
        assertEquals("Discovering", SenderStatusCodes.label(SenderStatusCodes.DISCOVERING))
        assertEquals("Pairing", SenderStatusCodes.label(SenderStatusCodes.PAIRING))
        assertEquals("Connecting", SenderStatusCodes.label(SenderStatusCodes.CONNECTING))
        assertEquals("Negotiating", SenderStatusCodes.label(SenderStatusCodes.NEGOTIATING))
        assertEquals("Streaming", SenderStatusCodes.label(SenderStatusCodes.STREAMING))
        assertEquals("Reconnecting", SenderStatusCodes.label(SenderStatusCodes.RECONNECTING))
        assertEquals(
            "Permission Required",
            SenderStatusCodes.label(SenderStatusCodes.PERMISSION_REQUIRED),
        )
        assertEquals(
            "Network Unstable",
            SenderStatusCodes.label(SenderStatusCodes.NETWORK_UNSTABLE),
        )
    }
}
