package com.picoo.camera.pairing

import com.picoo.camera.jni.PicooNative
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.TimeZone

/**
 * Structured paired-device list for Devices tab — REQ-PICOO-PAIRING-005 / PUC-007.
 *
 * A→W V1: Sender stores Windows receivers; platform label is hardcoded `Windows`.
 */
object TrustedDeviceList {
    const val PLATFORM_WINDOWS: String = "Windows"

    fun load(handle: Long): List<PicooNative.TrustedDevice> {
        if (handle == 0L) return emptyList()
        val count = PicooNative.getTrustedDeviceCount(handle)
        if (count <= 0) return emptyList()
        val out = ArrayList<PicooNative.TrustedDevice>(count)
        for (index in 0 until count) {
            val device = PicooNative.getTrustedDevice(handle, index) ?: continue
            out.add(device)
        }
        return out
    }

    fun ids(devices: List<PicooNative.TrustedDevice>): Set<String> =
        devices.mapTo(linkedSetOf()) { it.deviceId }

    /**
     * Wipe all paired devices and persist. Returns the number removed, or a
     * negative FFI/JNI error code.
     */
    fun clearAll(handle: Long): Int {
        if (handle == 0L) return 0
        val removed = PicooNative.clearTrustedDevices(handle)
        if (removed < 0) return removed
        val saveRc = PicooNative.saveTrustedStore(handle)
        if (saveRc != 0) return saveRc
        return removed
    }

    fun shortFingerprint(fingerprint: String, keep: Int = 8): String {
        if (fingerprint.isEmpty()) return "—"
        return if (fingerprint.length <= keep) fingerprint else fingerprint.take(keep) + "…"
    }

    /** UTC `yyyy-MM-dd` for last connect; em dash when unknown. */
    fun formatLastConnected(lastConnectedAtMs: Long): String {
        if (lastConnectedAtMs <= 0L) return "—"
        val fmt = SimpleDateFormat("yyyy-MM-dd", Locale.US)
        fmt.timeZone = TimeZone.getTimeZone("UTC")
        return fmt.format(Date(lastConnectedAtMs))
    }

    fun formatRow(device: PicooNative.TrustedDevice): String {
        val fp = shortFingerprint(device.certificateFingerprint)
        val last = formatLastConnected(device.lastConnectedAtMs)
        return "${device.deviceName} · $PLATFORM_WINDOWS · last $last · $fp"
    }
}
