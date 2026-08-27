package com.picoo.camera.pairing

import com.picoo.camera.jni.PicooNative
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class TrustedDeviceListTest {
    @Test
    fun shortFingerprintTruncates() {
        assertEquals("abcdef12…", TrustedDeviceList.shortFingerprint("abcdef1234567890"))
        assertEquals("ab", TrustedDeviceList.shortFingerprint("ab"))
        assertEquals("—", TrustedDeviceList.shortFingerprint(""))
    }

    @Test
    fun formatRowIncludesNameAndFingerprint() {
        val device = PicooNative.TrustedDevice(
            deviceId = "win-1",
            deviceName = "Office PC",
            certificateFingerprint = "deadbeefcafebabe",
            pairedAtMs = 1L,
            lastConnectedAtMs = 0L,
        )
        val row = TrustedDeviceList.formatRow(device)
        assertTrue(row.contains("Office PC"))
        assertTrue(row.contains("deadbeef…") || row.contains("deadbeef"))
    }

    @Test
    fun idsCollectsDeviceIds() {
        val devices = listOf(
            PicooNative.TrustedDevice("a", "A", "fp", 0, 0),
            PicooNative.TrustedDevice("b", "B", "fp", 0, 0),
        )
        assertEquals(setOf("a", "b"), TrustedDeviceList.ids(devices))
    }
}
