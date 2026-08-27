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
    fun formatLastConnectedUtcDateOrDash() {
        assertEquals("—", TrustedDeviceList.formatLastConnected(0L))
        // 2020-01-01T00:00:00Z
        assertEquals("2020-01-01", TrustedDeviceList.formatLastConnected(1_577_836_800_000L))
    }

    @Test
    fun formatRowIncludesPlatformLastConnectAndFingerprint() {
        val device = PicooNative.TrustedDevice(
            deviceId = "win-1",
            deviceName = "Office PC",
            certificateFingerprint = "deadbeefcafebabe",
            pairedAtMs = 1L,
            lastConnectedAtMs = 1_577_836_800_000L,
        )
        val row = TrustedDeviceList.formatRow(device)
        assertTrue(row.contains("Office PC"))
        assertTrue(row.contains("Windows"))
        assertTrue(row.contains("2020-01-01"))
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

    @Test
    fun clearAllWithZeroHandleIsNoop() {
        assertEquals(0, TrustedDeviceList.clearAll(0L))
    }
}
