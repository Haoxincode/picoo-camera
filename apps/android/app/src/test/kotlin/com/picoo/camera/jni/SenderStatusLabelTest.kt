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
        assertEquals("Disconnected", PicooNative.statusLabel(PicooNative.STATUS_DISCONNECTED))
        assertEquals("Discovering", PicooNative.statusLabel(PicooNative.STATUS_DISCOVERING))
        assertEquals("Pairing", PicooNative.statusLabel(PicooNative.STATUS_PAIRING))
        assertEquals("Connecting", PicooNative.statusLabel(PicooNative.STATUS_CONNECTING))
        assertEquals("Negotiating", PicooNative.statusLabel(PicooNative.STATUS_NEGOTIATING))
        assertEquals("Streaming", PicooNative.statusLabel(PicooNative.STATUS_STREAMING))
        assertEquals("Reconnecting", PicooNative.statusLabel(PicooNative.STATUS_RECONNECTING))
        assertEquals(
            "Permission Required",
            PicooNative.statusLabel(PicooNative.STATUS_PERMISSION_REQUIRED),
        )
        assertEquals(
            "Network Unstable",
            PicooNative.statusLabel(PicooNative.STATUS_NETWORK_UNSTABLE),
        )
    }
}
