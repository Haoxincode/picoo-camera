package com.picoo.camera.network

import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.wifi.WifiManager
import android.os.Build

/** REQ-PICOO-UI-0001 AC-M-DISC-01 — live Wi‑Fi SSID for discovery header pill. */
object WifiNetworkInfo {
    fun ssidLabel(context: Context): String {
        val app = context.applicationContext
        val wifi = app.getSystemService(Context.WIFI_SERVICE) as? WifiManager
        val ssid = wifi?.connectionInfo?.ssid?.trim('"').orEmpty()
        if (ssid.isNotBlank() && ssid != "<unknown ssid>") {
            return ssid
        }
        val cm = app.getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager
        val network = cm?.activeNetwork
        val caps = network?.let { cm.getNetworkCapabilities(it) }
        if (caps?.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) == true) {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                return "Wi‑Fi"
            }
        }
        return "局域网"
    }

    fun pillText(context: Context): String = "Wi‑Fi · ${ssidLabel(context)}"
}
