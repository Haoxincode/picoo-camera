package com.picoo.camera.runtime

import android.content.Context
import com.picoo.camera.jni.PicooNative
import com.picoo.camera.pairing.TrustedDeviceList
import java.io.Closeable

/**
 * Stable owner for native resources whose lifetime is the Sender screen.
 *
 * Handles never live in Compose state: reloading the trusted-device store can
 * therefore not restart the root DisposableEffect or destroy unrelated FFI
 * resources (REQ-PICOO-UI-010 / REQ-PICOO-PAIRING-004).
 */
class SenderNativeRuntime(context: Context) : Closeable {
    val trustedStorePath = java.io.File(context.filesDir, "trusted_devices.json").absolutePath
    val identityPath = java.io.File(context.filesDir, "sender_identity.json").absolutePath

    val identityHandle: Long = PicooNative.loadOrCreateIdentity(identityPath, android.os.Build.MODEL)
    val senderHandle: Long = PicooNative.createSender()

    private var trustedStoreHandle: Long = 0L
    private var senderTrustedStoreAttached = false
    private val autoConnectAttemptedIds = mutableSetOf<String>()
    private var closed = false

    val senderDeviceId: String
        get() = if (identityHandle == 0L) {
            "android-sender"
        } else {
            PicooNative.getIdentityDeviceId(identityHandle)
        }

    val senderPublicKey: ByteArray
        get() = if (identityHandle == 0L) {
            byteArrayOf(1, 2, 3)
        } else {
            PicooNative.getIdentityPublicKey(identityHandle)
        }

    val senderDeviceName: String
        get() = if (identityHandle == 0L) {
            android.os.Build.MODEL
        } else {
            PicooNative.getIdentityDeviceName(identityHandle).ifBlank { android.os.Build.MODEL }
        }

    fun attachTrustedStore(): Int {
        if (senderHandle == 0L) return -1
        val result = PicooNative.attachTrustedStore(senderHandle, trustedStorePath)
        senderTrustedStoreAttached = result == 0
        return result
    }

    fun reloadTrustedDevices(): List<PicooNative.TrustedDevice>? {
        check(!closed) { "sender runtime is closed" }
        val replacement = PicooNative.loadTrustedStore(trustedStorePath)
        if (replacement == 0L) return null
        val previous = trustedStoreHandle
        trustedStoreHandle = replacement
        if (previous != 0L) {
            PicooNative.destroyTrustedStore(previous)
        }
        return TrustedDeviceList.load(replacement)
    }

    fun removeTrustedDevice(deviceId: String): Int {
        if (senderHandle == 0L) return -1
        if (!senderTrustedStoreAttached) return -3
        return PicooNative.removeTrustedDevice(senderHandle, deviceId)
    }

    fun autoConnectAttempts(): Set<String> = autoConnectAttemptedIds.toSet()

    fun markAutoConnectAttempt(deviceId: String) {
        autoConnectAttemptedIds += deviceId
    }

    fun forgetAutoConnectAttempt(deviceId: String) {
        autoConnectAttemptedIds -= deviceId
    }

    fun clearAutoConnectAttempts() {
        autoConnectAttemptedIds.clear()
    }

    override fun close() {
        if (closed) return
        closed = true
        val store = trustedStoreHandle
        trustedStoreHandle = 0L
        if (store != 0L) PicooNative.destroyTrustedStore(store)
        if (senderHandle != 0L) PicooNative.destroySender(senderHandle)
        if (identityHandle != 0L) PicooNative.destroyIdentity(identityHandle)
    }
}
