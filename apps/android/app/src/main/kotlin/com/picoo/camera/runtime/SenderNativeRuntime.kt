package com.picoo.camera.runtime

import android.content.Context
import android.net.ConnectivityManager
import android.net.InetAddresses
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import com.picoo.camera.jni.PicooNative
import com.picoo.camera.pairing.TrustedDeviceList
import java.io.Closeable
import java.net.InetAddress
import java.util.concurrent.atomic.AtomicReference

internal fun addressIsInPrefix(
    destination: InetAddress,
    prefixAddress: InetAddress,
    prefixLength: Int,
): Boolean {
    val destinationBytes = destination.address
    val prefixBytes = prefixAddress.address
    if (destinationBytes.size != prefixBytes.size || prefixLength !in 0..prefixBytes.size * 8) {
        return false
    }
    val wholeBytes = prefixLength / 8
    for (index in 0 until wholeBytes) {
        if (destinationBytes[index] != prefixBytes[index]) return false
    }
    val remainingBits = prefixLength % 8
    if (remainingBits == 0) return true
    val mask = (0xff shl (8 - remainingBits)) and 0xff
    return (destinationBytes[wholeBytes].toInt() and mask) ==
        (prefixBytes[wholeBytes].toInt() and mask)
}

enum class QuicWifiBindingResult {
    Bound,
    WifiUnavailable,
    Blocked,
}

/**
 * Stable owner for native resources whose lifetime is the Sender screen.
 *
 * Handles never live in Compose state: reloading the trusted-device store can
 * therefore not restart the root DisposableEffect or destroy unrelated FFI
 * resources (REQ-PICOO-UI-010 / REQ-PICOO-PAIRING-004).
 */
class SenderNativeRuntime(context: Context) : Closeable {
    private val connectivityManager =
        context.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager

    val trustedStorePath = java.io.File(context.filesDir, "trusted_devices.json").absolutePath
    val identityHandle: Long = createIdentityHandle(context)
    val senderHandle: Long = PicooNative.createSender(identityHandle)

    private var trustedStoreHandle: Long = 0L
    private var senderTrustedStoreAttached = false
    private data class WifiRouteConfig(
        val networkHandle: Long,
        val allowSystemLanRouteFallback: Boolean,
    )

    private var configuredWifiRoute: WifiRouteConfig? = null
    private val wifiNetwork = AtomicReference<Network?>(null)
    private val wifiNetworkCallback = object : ConnectivityManager.NetworkCallback() {
        override fun onAvailable(network: Network) {
            wifiNetwork.set(network)
        }

        override fun onLost(network: Network) {
            wifiNetwork.compareAndSet(network, null)
        }
    }
    private var wifiNetworkCallbackRegistered = false
    private val autoConnectAttemptedIds = mutableSetOf<String>()
    private var closed = false

    init {
        val request = NetworkRequest.Builder()
            .addTransportType(NetworkCapabilities.TRANSPORT_WIFI)
            .addCapability(NetworkCapabilities.NET_CAPABILITY_NOT_VPN)
            .build()
        wifiNetworkCallbackRegistered = runCatching {
            connectivityManager.registerNetworkCallback(request, wifiNetworkCallback)
            true
        }.getOrDefault(false)
    }

    fun attachTrustedStore(): Int {
        if (senderHandle == 0L) return -1
        if (senderTrustedStoreAttached) return 0
        val result = PicooNative.attachTrustedStore(senderHandle, trustedStorePath)
        senderTrustedStoreAttached = result == 0
        return result
    }

    /**
     * Keep local QUIC sockets on the physical Wi-Fi network even when Android's default network
     * is a VPN. Picoo is LAN-only, so a VPN-owned process route must never become its transport
     * boundary (REQ-PICOO-DISCOVERY-007 / REQ-PICOO-TRANSPORT-010).
     */
    fun configureQuicWifiNetwork(endpointHost: String): QuicWifiBindingResult {
        if (closed) return QuicWifiBindingResult.Blocked
        if (!wifiNetworkCallbackRegistered) return QuicWifiBindingResult.Blocked
        val currentWifiNetwork = wifiNetwork.get() ?: return QuicWifiBindingResult.WifiUnavailable
        val route = WifiRouteConfig(
            networkHandle = currentWifiNetwork.networkHandle,
            allowSystemLanRouteFallback = systemLanRouteFallbackIsSafe(
                endpointHost,
                currentWifiNetwork,
            ),
        )
        if (route == configuredWifiRoute) {
            return QuicWifiBindingResult.Bound
        }
        return if (runCatching {
                PicooNative.setNetworkHandle(
                    senderHandle,
                    route.networkHandle,
                    route.allowSystemLanRouteFallback,
                )
            }.getOrDefault(-1) == 0
        ) {
            configuredWifiRoute = route
            QuicWifiBindingResult.Bound
        } else {
            QuicWifiBindingResult.Blocked
        }
    }

    /**
     * A VPN that did not call `VpnService.Builder.allowBypass()` rejects per-socket network
     * binding. Split-tunnel VPNs can still deliberately exclude the directly connected LAN from
     * their routes. In that narrow case Android's ordinary route is the physical Wi-Fi route, so
     * Quinn may keep the unbound socket after `android_setsocknetwork` is rejected.
     */
    private fun systemLanRouteFallbackIsSafe(
        endpointHost: String,
        currentWifiNetwork: Network,
    ): Boolean {
        val numericHost = endpointHost.removePrefix("[").removeSuffix("]").substringBefore('%')
        if (!InetAddresses.isNumericAddress(numericHost)) return false
        val destination = runCatching { InetAddresses.parseNumericAddress(numericHost) }
            .getOrNull() ?: return false
        val wifiProperties = connectivityManager.getLinkProperties(currentWifiNetwork) ?: return false
        val directlyConnected = wifiProperties.linkAddresses.any { linkAddress ->
            addressIsInPrefix(destination, linkAddress.address, linkAddress.prefixLength)
        }
        if (!directlyConnected) return false

        val defaultNetwork = connectivityManager.activeNetwork ?: return false
        if (defaultNetwork == currentWifiNetwork) return true
        val defaultCapabilities = connectivityManager.getNetworkCapabilities(defaultNetwork)
            ?: return false
        if (!defaultCapabilities.hasTransport(NetworkCapabilities.TRANSPORT_VPN)) return false
        val vpnProperties = connectivityManager.getLinkProperties(defaultNetwork) ?: return false
        return vpnProperties.routes.none { route -> route.matches(destination) }
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
        if (wifiNetworkCallbackRegistered) {
            runCatching { connectivityManager.unregisterNetworkCallback(wifiNetworkCallback) }
            wifiNetworkCallbackRegistered = false
        }
        wifiNetwork.set(null)
        configuredWifiRoute = null
        val store = trustedStoreHandle
        trustedStoreHandle = 0L
        if (store != 0L) PicooNative.destroyTrustedStore(store)
        if (senderHandle != 0L) PicooNative.destroySender(senderHandle)
        if (identityHandle != 0L) PicooNative.destroyIdentity(identityHandle)
    }

    private companion object {
        fun createIdentityHandle(context: Context): Long {
            val secret = AndroidIdentityStore(context).load()
            val handle = try {
                PicooNative.loadIdentityFromSecret(secret, android.os.Build.MODEL)
            } finally {
                secret.fill(0)
            }
            check(handle != 0L) { "secure sender identity is unavailable" }
            // Removed plaintext identity state is never imported as trust evidence.
            java.io.File(context.filesDir, "sender_identity.json").delete()
            return handle
        }
    }
}
