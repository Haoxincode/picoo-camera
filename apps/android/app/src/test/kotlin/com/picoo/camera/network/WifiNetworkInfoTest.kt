package com.picoo.camera.network

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** REQ-PICOO-UI-0001 AC-M-DISC-01 — SSID normalization helpers. */
class WifiNetworkInfoTest {
    @Test
    fun normalizeSsid_stripsQuotesAndRejectsUnknown() {
        assertEquals("Office-WiFi", WifiNetworkInfo.normalizeSsid("\"Office-WiFi\""))
        assertEquals("Home", WifiNetworkInfo.normalizeSsid("Home"))
        assertNull(WifiNetworkInfo.normalizeSsid(null))
        assertNull(WifiNetworkInfo.normalizeSsid(""))
        assertNull(WifiNetworkInfo.normalizeSsid("<unknown ssid>"))
        assertNull(WifiNetworkInfo.normalizeSsid("\"<unknown ssid>\""))
    }

    @Test
    fun fallbackLabel_wifiVsLan() {
        assertEquals("Wi‑Fi", WifiNetworkInfo.fallbackLabel(hasWifiTransport = true))
        assertEquals("局域网", WifiNetworkInfo.fallbackLabel(hasWifiTransport = false))
    }

    @Test
    fun pillText_formatsPrefix() {
        assertEquals("Wi‑Fi · Office", WifiNetworkInfo.formatPill("Office"))
    }
}
