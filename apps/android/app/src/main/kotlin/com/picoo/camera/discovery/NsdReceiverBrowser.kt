package com.picoo.camera.discovery

import android.content.Context
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.net.wifi.WifiManager
import android.os.Build
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
    private val wifiNetwork: () -> Network? = { null },
    private val onChanged: (List<PicooNative.DiscoveredReceiver>) -> Unit,
) {
    private val appContext = context.applicationContext
    private val nsdManager = appContext.getSystemService(Context.NSD_SERVICE) as NsdManager
    private val mainHandler = Handler(Looper.getMainLooper())
    private val receivers = ConcurrentHashMap<String, PicooNative.DiscoveredReceiver>()
    private val receiverIdsByServiceName = ConcurrentHashMap<String, String>()
    private val pendingLosses = ConcurrentHashMap<String, Runnable>()
    private var discoveryListener: NsdManager.DiscoveryListener? = null
    private var multicastLock: WifiManager.MulticastLock? = null
    private var started = false
    private var desired = false
    private var restartRunnable: Runnable? = null

    fun start() {
        desired = true
        startDiscovery()
    }

    private fun startDiscovery() {
        if (started || !desired) return
        restartRunnable?.let(mainHandler::removeCallbacks)
        restartRunnable = null
        started = true
        acquireMulticastLock()
        val listener =
            object : NsdManager.DiscoveryListener {
                override fun onDiscoveryStarted(serviceType: String) {
                    Log.i(TAG, "NSD discovery started: $serviceType")
                }

                override fun onServiceFound(serviceInfo: NsdServiceInfo) {
                    Log.i(TAG, "NSD found ${serviceInfo.serviceName} type=${serviceInfo.serviceType}")
                    if (!serviceInfo.serviceType.contains("picoocam", ignoreCase = true)) {
                        return
                    }
                    resolve(serviceInfo)
                }

                override fun onServiceLost(serviceInfo: NsdServiceInfo) {
                    // Android NSD can emit a transient loss while Wi-Fi/VPN routes settle or
                    // while the Receiver refreshes its TXT record after pairing. Keep the row
                    // briefly and cancel the removal if the same service resolves again.
                    scheduleLoss(serviceInfo.serviceName)
                }

                override fun onDiscoveryStopped(serviceType: String) {
                    Log.i(TAG, "NSD discovery stopped: $serviceType")
                    started = false
                    if (desired) scheduleRestart()
                }

                override fun onStartDiscoveryFailed(serviceType: String, errorCode: Int) {
                    Log.w(TAG, "NSD start failed: $errorCode")
                    started = false
                    releaseMulticastLock()
                    scheduleRestart()
                }

                override fun onStopDiscoveryFailed(serviceType: String, errorCode: Int) {
                    Log.w(TAG, "NSD stop failed: $errorCode")
                }
            }
        discoveryListener = listener
        runCatching {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                // Bind discovery to the physical Wi-Fi Network. A request that only
                // names TRANSPORT_WIFI also matches a VPN-over-Wi-Fi agent
                // (v2rayNG reports WIFI|VPN), so multicast never leaves tun0.
                val wifi = wifiNetwork()
                Log.i(TAG, "NSD bind wifi=${wifi ?: "request-not-vpn"}")
                if (wifi != null) {
                    nsdManager.discoverServices(
                        DiscoveryTxt.SERVICE_TYPE,
                        NsdManager.PROTOCOL_DNS_SD,
                        wifi,
                        appContext.mainExecutor,
                        listener,
                    )
                } else {
                    nsdManager.discoverServices(
                        DiscoveryTxt.SERVICE_TYPE,
                        NsdManager.PROTOCOL_DNS_SD,
                        WIFI_NOT_VPN_REQUEST,
                        appContext.mainExecutor,
                        listener,
                    )
                }
            } else {
                nsdManager.discoverServices(
                    DiscoveryTxt.SERVICE_TYPE,
                    NsdManager.PROTOCOL_DNS_SD,
                    listener,
                )
            }
        }.onFailure {
            Log.e(TAG, "discoverServices failed", it)
            started = false
            releaseMulticastLock()
            scheduleRestart()
        }
    }

    fun stop() {
        desired = false
        restartRunnable?.let(mainHandler::removeCallbacks)
        restartRunnable = null
        pendingLosses.values.forEach(mainHandler::removeCallbacks)
        pendingLosses.clear()
        val listener = discoveryListener
        discoveryListener = null
        if (listener != null) {
            runCatching { nsdManager.stopServiceDiscovery(listener) }
        }
        receivers.clear()
        receiverIdsByServiceName.clear()
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
                    pendingLosses.remove(resolved.serviceName)?.let(mainHandler::removeCallbacks)
                    val attrs = resolved.attributes ?: emptyMap()
                    val parsed = DiscoveryTxt.parseAttributes(attrs)
                    if (parsed == null) {
                        Log.w(
                            TAG,
                            "TXT rejected for ${resolved.serviceName} keys=${attrs.keys}",
                        )
                        return
                    }
                    val host =
                        resolved.host?.hostAddress
                            ?: resolved.host?.hostName
                    if (host == null) {
                        Log.w(TAG, "resolved ${resolved.serviceName} without a host address")
                        return
                    }
                    Log.i(TAG, "NSD resolved ${parsed.displayName} at $host:${parsed.quicPort}")
                    val entry =
                        PicooNative.DiscoveredReceiver(
                            receiverId = parsed.receiverId,
                            displayName = parsed.displayName,
                            host = host,
                            quicPort = parsed.quicPort,
                            platform = parsed.platform,
                            pairingState = parsed.pairingState,
                        )
                    receivers[parsed.receiverId] = entry
                    receiverIdsByServiceName[resolved.serviceName] = parsed.receiverId
                    publish()
                }
            },
        )
    }

    private fun publish() {
        val list = snapshot()
        mainHandler.post { onChanged(list) }
    }

    private fun scheduleLoss(serviceName: String) {
        pendingLosses.remove(serviceName)?.let(mainHandler::removeCallbacks)
        val removal = Runnable {
            pendingLosses.remove(serviceName)
            receiverIdsByServiceName.remove(serviceName)?.let(receivers::remove)
            publish()
        }
        pendingLosses[serviceName] = removal
        mainHandler.postDelayed(removal, SERVICE_LOSS_GRACE_MS)
    }

    private fun scheduleRestart() {
        if (!desired || restartRunnable != null) return
        val restart = Runnable {
            restartRunnable = null
            startDiscovery()
        }
        restartRunnable = restart
        mainHandler.postDelayed(restart, RESTART_DELAY_MS)
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
        private const val SERVICE_LOSS_GRACE_MS = 10_000L
        private const val RESTART_DELAY_MS = 1_500L
        private val WIFI_NOT_VPN_REQUEST: NetworkRequest =
            NetworkRequest.Builder()
                .addTransportType(NetworkCapabilities.TRANSPORT_WIFI)
                .addCapability(NetworkCapabilities.NET_CAPABILITY_NOT_VPN)
                .build()
    }
}
