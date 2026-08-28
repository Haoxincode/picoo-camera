package com.picoo.camera.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** REQ-PICOO-UI-0001 AC-M-LIVE-08 / PUC-006 reconnect backoff labels. */
class ReconnectBackoffFormatTest {
    @Test
    fun delayLabel_matchesPrototypeLadder() {
        assertEquals("500ms", ReconnectBackoffFormat.delayLabel(500))
        assertEquals("1s", ReconnectBackoffFormat.delayLabel(1_000))
        assertEquals("2s", ReconnectBackoffFormat.delayLabel(2_000))
        assertEquals("5s", ReconnectBackoffFormat.delayLabel(5_000))
    }

    @Test
    fun detailMessage_includesAttemptAndDelay() {
        val msg = ReconnectBackoffFormat.detailMessage(3, 2_000)
        assertTrue(msg.contains("第 3 次"))
        assertTrue(msg.contains("2s"))
        assertTrue(msg.contains("IDR"))
    }

    @Test
    fun detailMessage_fallbackWhenNotRetrying() {
        val msg = ReconnectBackoffFormat.detailMessage(0, 0)
        assertTrue(msg.contains("QUIC 会话重试中"))
        assertTrue(msg.contains("IDR"))
    }
}
