package com.picoo.camera.jni

/**
 * JNI bridge to Rust Core C ABI (REQ-PICOO-STACK-003).
 *
 * Kotlin → JNI (libpicoo_jni.so) → C ABI (libpicoo_ffi.so) → Rust
 */
object PicooNative {
    init {
        System.loadLibrary("picoo_ffi")
        System.loadLibrary("picoo_jni")
    }

    const val STATUS_DISCONNECTED = 0
    const val STATUS_DISCOVERING = 1
    const val STATUS_PAIRING = 2
    const val STATUS_CONNECTING = 3
    const val STATUS_NEGOTIATING = 4
    const val STATUS_STREAMING = 5
    const val STATUS_RECONNECTING = 6

    /** Returns PCP/1 protocol version from Rust Core. */
    external fun getProtocolVersion(): String

    /** Create Rust sender pipeline; returns opaque handle (0 = failure). */
    external fun createSender(): Long

    /** Destroy sender pipeline handle. */
    external fun destroySender(handle: Long)

    /**
     * Packetize one H.264 access unit into PCP/1 VideoPackets.
     * @return fragment count on success, negative on error.
     */
    external fun ingestAccessUnit(
        handle: Long,
        data: ByteArray,
        keyframe: Boolean,
        ptsUs: Long,
        streamEpoch: Int,
    ): Int

    /** [accessUnits, packets, bytes, sentDatagrams, pendingPackets] */
    external fun getSenderStats(handle: Long): LongArray

    external fun connect(handle: Long, host: String, port: Int): Int

    external fun flushPending(handle: Long): Int

    external fun pump(handle: Long): Int

    external fun getSenderStatus(handle: Long): Int

    external fun sendClientHello(
        handle: Long,
        senderId: String,
        deviceName: String,
        publicKey: ByteArray?,
    ): Int

    external fun sendPairingConfirm(handle: Long, receiverId: String): Int

    external fun getPairingShortCode(handle: Long): String

    external fun setStreamConfig(
        handle: Long,
        width: Int,
        height: Int,
        fps: Int,
        bitrateBps: Int,
        streamEpoch: Int,
        mirrored: Boolean,
        sps: ByteArray? = null,
        pps: ByteArray? = null,
    ): Int

    /** Adaptive bitrate target from ReceiverStats feedback (REQ-PICOO-MEDIA-007). */
    external fun getCurrentBitrateBps(handle: Long): Int

    /** Max height from receiver Capabilities; 0 if unknown (REQ-PICOO-MEDIA-002). */
    external fun getReceiverMaxHeight(handle: Long): Int

    /** 1 if receiver requested IDR (consumes flag). REQ-PICOO-SESSION-003. */
    external fun takeKeyframeRequest(handle: Long): Int

    /**
     * Extract SPS/PPS from Annex-B or AVCC codec-config bytes.
     * @return `[sps, pps]` or null when extraction fails.
     */
    external fun extractSpsPps(data: ByteArray): Array<ByteArray>?

    external fun createDiscoveryBrowser(): Long

    external fun destroyDiscoveryBrowser(handle: Long)

    external fun pollDiscoveryBrowser(handle: Long, timeoutMs: Int): Int

    external fun getDiscoveryCount(handle: Long): Int

    external fun getDiscoveredReceiver(handle: Long, index: Int): DiscoveredReceiver?

    external fun attachTrustedStore(handle: Long, path: String): Int

    external fun getConnectedReceiverId(handle: Long): String

    external fun loadTrustedStore(path: String): Long

    external fun destroyTrustedStore(handle: Long)

    external fun getTrustedDeviceCount(handle: Long): Int

    external fun getTrustedDevice(handle: Long, index: Int): TrustedDevice?

    external fun removeTrustedDevice(handle: Long, deviceId: String): Int

    external fun saveTrustedStore(handle: Long): Int

    /** Load or create durable sender identity (REQ-PICOO-PAIRING-001). */
    external fun loadOrCreateIdentity(path: String, defaultName: String): Long

    external fun destroyIdentity(handle: Long)

    external fun getIdentityDeviceId(handle: Long): String

    external fun getIdentityDeviceName(handle: Long): String

    external fun getIdentityPublicKey(handle: Long): ByteArray

    external fun exportDiagnosticsToPath(
        trustedStorePath: String,
        platform: String,
        appVersion: String,
        outPath: String,
    ): Int

    external fun parseQrConnect(json: String): QrConnectPayload?

    data class SenderStats(
        val accessUnits: Long,
        val packets: Long,
        val bytes: Long,
        val sentDatagrams: Long,
        val pendingPackets: Long,
    )

    data class DiscoveredReceiver(
        val receiverId: String,
        val displayName: String,
        val host: String,
        val quicPort: Int,
    )

    data class TrustedDevice(
        val deviceId: String,
        val deviceName: String,
        val certificateFingerprint: String,
        val pairedAtMs: Long,
        val lastConnectedAtMs: Long,
    )

    data class QrConnectPayload(
        val host: String,
        val quicPort: Int,
        val receiverId: String,
        val expiresAtMs: Long,
    )

    fun readSenderStats(handle: Long): SenderStats {
        val values = getSenderStats(handle)
        return SenderStats(
            accessUnits = values.getOrElse(0) { 0 },
            packets = values.getOrElse(1) { 0 },
            bytes = values.getOrElse(2) { 0 },
            sentDatagrams = values.getOrElse(3) { 0 },
            pendingPackets = values.getOrElse(4) { 0 },
        )
    }

    fun statusLabel(status: Int): String =
        when (status) {
            STATUS_DISCOVERING -> "Discovering"
            STATUS_PAIRING -> "Pairing"
            STATUS_CONNECTING -> "Connecting"
            STATUS_NEGOTIATING -> "Negotiating"
            STATUS_STREAMING -> "Streaming"
            STATUS_RECONNECTING -> "Reconnecting"
            else -> "Disconnected"
        }
}
