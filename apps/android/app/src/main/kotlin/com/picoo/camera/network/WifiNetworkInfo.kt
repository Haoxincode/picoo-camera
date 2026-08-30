package com.picoo.camera.network

import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.wifi.WifiManager

/** REQ-PICOO-UI-0001 AC-M-DISC-01 — live Wi‑Fi SSID for discovery header pill. */
object WifiNetworkInfo {
    /** Pure helper for JVM tests / unknown-SSID filtering. */
    fun normalizeSsid(raw: String?): String? {
        val ssid = raw?.trim()?.trim('"').orEmpty()
        if (ssid.isBlank() || ssid == "<unknown ssid>") {
            return null
        }
        return ssid
    }

    fun fallbackLabel(hasWifiTransport: Boolean): String =
        if (hasWifiTransport) "Wi‑Fi" else "局域网"

    fun formatPill(ssidOrFallback: String): String = when (ssidOrFallback) {
        "Wi‑Fi", "局域网" -> ssidOrFallback
        else -> "Wi‑Fi · $ssidOrFallback"
    }

    fun ssidLabel(context: Context): String {
        val app = context.applicationContext
        val wifi = app.getSystemService(Context.WIFI_SERVICE) as? WifiManager
        normalizeSsid(wifi?.connectionInfo?.ssid)?.let { return it }
        val cm = app.getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager
        val network = cm?.activeNetwork
        val caps = network?.let { cm.getNetworkCapabilities(it) }
        val hasWifi = caps?.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) == true
        return fallbackLabel(hasWifi)
    }

    fun pillText(context: Context): String = formatPill(ssidLabel(context))
}
