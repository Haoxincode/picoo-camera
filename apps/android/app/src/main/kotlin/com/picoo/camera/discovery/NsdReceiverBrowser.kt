package com.picoo.camera.discovery

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.net.wifi.WifiManager
import android.os.Handler
import android.os.Looper
import android.util.Log
import com.picoo.camera.jni.PicooNative
import java.util.concurrent.ConcurrentHashMap

/**
 * Android NSD / DNS-SD browser for Picoo Camera receivers (REQ-PICOO-DISCOVERY-005).
 *
 * Replaces Rust mDNS browse on Android per ARCH-PICOO-DISCOVERY-001.
 * Connect remains Kotlin → Rust JNI → QUIC (no Quinn types in this class).
 */
class NsdReceiverBrowser(
    context: Context,
    private val onChanged: (List<PicooNative.DiscoveredReceiver>) -> Unit,
) {
    private val appContext = context.applicationContext
    private val nsdManager = appContext.getSystemService(Context.NSD_SERVICE) as NsdManager
    private val mainHandler = Handler(Looper.getMainLooper())
    private val receivers = ConcurrentHashMap<String, PicooNative.DiscoveredReceiver>()
    private var discoveryListener: NsdManager.DiscoveryListener? = null
    private var multicastLock: WifiManager.MulticastLock? = null
    private var started = false

    fun start() {
        if (started) return
        started = true
        acquireMulticastLock()
        val listener =
            object : NsdManager.DiscoveryListener {
                override fun onDiscoveryStarted(serviceType: String) {
                    Log.i(TAG, "NSD discovery started: $serviceType")
                }

                override fun onServiceFound(serviceInfo: NsdServiceInfo) {
                    if (!serviceInfo.serviceType.contains("picoocam", ignoreCase = true)) {
                        return
                    }
                    resolve(serviceInfo)
                }

                override fun onServiceLost(serviceInfo: NsdServiceInfo) {
                    val key = serviceInfo.serviceName
                    receivers.remove(key)
                    // Also drop by receiver_id if present in name.
                    receivers.entries.removeIf { it.value.displayName == serviceInfo.serviceName }
                    publish()
                }

                override fun onDiscoveryStopped(serviceType: String) {
                    Log.i(TAG, "NSD discovery stopped: $serviceType")
                }

                override fun onStartDiscoveryFailed(serviceType: String, errorCode: Int) {
                    Log.w(TAG, "NSD start failed: $errorCode")
                    started = false
                }

                override fun onStopDiscoveryFailed(serviceType: String, errorCode: Int) {
                    Log.w(TAG, "NSD stop failed: $errorCode")
                }
            }
        discoveryListener = listener
        runCatching {
            nsdManager.discoverServices(
                DiscoveryTxt.SERVICE_TYPE,
                NsdManager.PROTOCOL_DNS_SD,
                listener,
            )
        }.onFailure {
            Log.e(TAG, "discoverServices failed", it)
            started = false
            releaseMulticastLock()
        }
    }

    fun stop() {
        val listener = discoveryListener
        discoveryListener = null
        if (listener != null) {
            runCatching { nsdManager.stopServiceDiscovery(listener) }
        }
        receivers.clear()
        started = false
        releaseMulticastLock()
        publish()
    }

    fun snapshot(): List<PicooNative.DiscoveredReceiver> =
        receivers.values.sortedBy { it.displayName.lowercase() }

    private fun resolve(serviceInfo: NsdServiceInfo) {
        nsdManager.resolveService(
            serviceInfo,
            object : NsdManager.ResolveListener {
                override fun onResolveFailed(serviceInfo: NsdServiceInfo, errorCode: Int) {
                    Log.w(TAG, "resolve failed for ${serviceInfo.serviceName}: $errorCode")
                }

                override fun onServiceResolved(resolved: NsdServiceInfo) {
                    val attrs = resolved.attributes ?: emptyMap()
                    val parsed = DiscoveryTxt.parseAttributes(attrs) ?: return
                    val host =
                        resolved.host?.hostAddress
                            ?: resolved.host?.hostName
                            ?: return
                    val entry =
                        PicooNative.DiscoveredReceiver(
                            receiverId = parsed.receiverId,
                            displayName = parsed.displayName,
                            host = host,
                            quicPort = parsed.quicPort,
                            pairingState = parsed.pairingState,
                        )
                    receivers[parsed.receiverId] = entry
                    publish()
                }
            },
        )
    }

    private fun publish() {
        val list = snapshot()
        mainHandler.post { onChanged(list) }
    }

    private fun acquireMulticastLock() {
        if (multicastLock != null) return
        val wifi = appContext.getSystemService(Context.WIFI_SERVICE) as? WifiManager ?: return
        multicastLock =
            wifi.createMulticastLock("picoo-nsd").apply {
                setReferenceCounted(true)
                acquire()
            }
    }

    private fun releaseMulticastLock() {
        runCatching { multicastLock?.release() }
        multicastLock = null
    }

    companion object {
        private const val TAG = "NsdReceiverBrowser"
    }
}
