package com.picoo.camera.ui

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** REQ-PICOO-DISCOVERY-003 — 扫码识别不得因重复帧/重组再次进入连接。 */
class QrScanSessionTest {
    @Test
    fun firstNonBlankPayloadIsEmittedOnce() {
        val session = QrScanSession()
        assertTrue(session.tryEmit("""{"v":1,"host":"10.0.0.8"}"""))
        assertFalse(session.shouldAnalyze())
        assertFalse(session.tryEmit("""{"v":1,"host":"10.0.0.8"}"""))
        session.finishEmit()
        assertTrue(session.shouldAnalyze())
        assertFalse(session.tryEmit("""{"v":1,"host":"10.0.0.8"}"""))
    }

    @Test
    fun differentPayloadCanFollowInvalidOrExpiredCode() {
        val session = QrScanSession()
        assertTrue(session.tryEmit("expired-qr"))
        session.finishEmit()
        assertTrue(session.tryEmit("""{"v":1,"host":"10.0.0.8"}"""))
    }

    @Test
    fun blankPayloadIsIgnored() {
        val session = QrScanSession()
        assertFalse(session.tryEmit(""))
        assertFalse(session.tryEmit("   "))
        assertTrue(session.shouldAnalyze())
        assertTrue(session.tryEmit("ok"))
    }

    @Test
    fun concurrentEmitIsRejectedUntilFinished() {
        val session = QrScanSession()
        assertTrue(session.tryEmit("first"))
        assertFalse(session.tryEmit("second"))
        session.finishEmit()
        assertTrue(session.tryEmit("second"))
    }
}
