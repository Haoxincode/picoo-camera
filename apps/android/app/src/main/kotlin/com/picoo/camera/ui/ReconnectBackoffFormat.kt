package com.picoo.camera.ui

/** REQ-PICOO-UI-0001 AC-M-LIVE-08 — align prototype reconnect backoff labels. */
object ReconnectBackoffFormat {
    fun delayLabel(ms: Long): String = when (ms) {
        500L -> "500ms"
        1_000L -> "1s"
        2_000L -> "2s"
        5_000L -> "5s"
        else -> if (ms >= 1_000) "${ms / 1_000}s" else "${ms}ms"
    }

    fun detailMessage(attempt: Int, delayMs: Long): String {
        if (attempt <= 0 || delayMs <= 0L) {
            return "QUIC 会话重试中\n连接恢复后将自动请求 IDR 关键帧"
        }
        return "QUIC 会话重试中 · 第 ${attempt} 次 (${delayLabel(delayMs)})\n连接恢复后将自动请求 IDR 关键帧"
    }
}
