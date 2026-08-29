package com.picoo.camera.ui

import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference

/**
 * PUC-003 / REQ-PICOO-DISCOVERY-003 — 扫码会话门闩。
 *
 * CameraX 绑定由 [QrCodeScanner] 的 `AndroidView.factory` 负责，且只应发生一次。
 * 本对象只约束识别回调：同一帧流上的重复 QR 不得再次进入连接，
 * 也不把“已识别”写进 Compose state，以免扫描线动画牵动重组后再次 `bindToLifecycle`。
 */
internal class QrScanSession {
    private val lastEmitted = AtomicReference<String?>(null)
    private val emitting = AtomicBoolean(false)

    fun shouldAnalyze(): Boolean = !emitting.get()

    /**
     * 接受下一条待投递的 payload。空白、正在投递中、或与上一条相同的内容都会被忽略，
     * 这样过期/无效码不会永久锁死取景，用户仍可扫下一张不同的码。
     */
    fun tryEmit(payload: String): Boolean {
        val trimmed = payload.trim()
        if (trimmed.isEmpty()) return false
        if (lastEmitted.get() == trimmed) return false
        if (!emitting.compareAndSet(false, true)) return false
        lastEmitted.set(trimmed)
        return true
    }

    fun finishEmit() {
        emitting.set(false)
    }
}
