package com.picoo.camera.jni

/**
 * JNI bridge to Rust Core C ABI (REQ-PICOO-STACK-003).
 *
 * Kotlin → Rust JNI exports (libpicoo_ffi.so) → Rust Core
 *
 * Native load is explicit and recoverable: a failed `<clinit>` would otherwise
 * poison the class with ExceptionInInitializerError and crash Compose startup
 * on devices that cannot dlopen (e.g. 16 KB page size).
 */
object PicooNative {
    @Volatile
    private var loadAttempted: Boolean = false

    /** Null when native libs loaded successfully; otherwise the failure message. */
    @Volatile
    var loadError: String? = null
        private set

    val isAvailable: Boolean
        get() = ensureLoaded()

    /**
     * Load the single Rust/JNI library. Safe to call repeatedly.
     * @return true when the library loaded.
     */
    @Synchronized
    fun ensureLoaded(): Boolean {
        if (loadAttempted) {
            return loadError == null
        }
        loadAttempted = true
        return try {
            System.loadLibrary("picoo_ffi")
            loadError = null
            true
        } catch (t: Throwable) {
            loadError = t.message ?: t.toString()
            false
        }
    }

    const val STATUS_DISCONNECTED = SenderStatusCodes.DISCONNECTED
    const val STATUS_DISCOVERING = SenderStatusCodes.DISCOVERING
    const val STATUS_PAIRING = SenderStatusCodes.PAIRING
    const val STATUS_CONNECTING = SenderStatusCodes.CONNECTING
    const val STATUS_NEGOTIATING = SenderStatusCodes.NEGOTIATING
    const val STATUS_STREAMING = SenderStatusCodes.STREAMING
    const val STATUS_RECONNECTING = SenderStatusCodes.RECONNECTING
    const val STATUS_PERMISSION_REQUIRED = SenderStatusCodes.PERMISSION_REQUIRED
    const val STATUS_NETWORK_UNSTABLE = SenderStatusCodes.NETWORK_UNSTABLE

    /** Returns the unversioned PCP protocol name from Rust Core. */
    external fun getProtocolName(): String

    /** Create Rust sender pipeline; returns opaque handle (0 = failure). */
    external fun createSender(identityHandle: Long): Long

    /** Destroy sender pipeline handle. */
    external fun destroySender(handle: Long)

    /**
     * Packetize one H.264 access unit into PCP FEC-protected VideoPackets.
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

    /** User-initiated stop; does not auto-reconnect until the next connect (PUC-005). */
    external fun disconnect(handle: Long): Int

    external fun flushPending(handle: Long): Int

    external fun pump(handle: Long): Int

    /** Coherent Rust sender state; prefer this to combining independent getters. */
    external fun getSenderSnapshot(handle: Long): LongArray

    /** REQ-PICOO-SESSION-001: surface Permission Required when CAMERA denied. */
    external fun markPermissionRequired(handle: Long): Int

    /** REQ-PICOO-SESSION-001: clear Permission Required after CAMERA granted. */
    external fun clearPermissionRequired(handle: Long): Int

    external fun sendClientHello(handle: Long): Int

    external fun sendPairingConfirm(handle: Long, receiverId: String): Int

    external fun getPairingShortCode(handle: Long): String

    external fun setStreamConfig(
        handle: Long,
        width: Int,
        height: Int,
        fps: Int,
        bitrateBps: Int,
        mirrored: Boolean,
        rotation: Int = 0,
        sps: ByteArray? = null,
        pps: ByteArray? = null,
    ): Int

    /**
     * Latest ReceiverStats feedback for Streaming metrics (PUC-005).
     * @return [rttMs, packetLoss, jitterMs, frameAgeMs, receiveBitrate, jitterDepthMs] or null.
     */
    external fun getLinkStats(handle: Long): DoubleArray?

    /** 1 if receiver requested IDR (consumes flag). REQ-PICOO-SESSION-003. */
    external fun takeKeyframeRequest(handle: Long): Int

    /**
     * Desktop CameraCommand (PUC-005 / REQ-PICOO-UI-009).
     * @return 0=none, 1=SWITCH_FRONT, 2=SWITCH_BACK, 3=SET_RESOLUTION, 4=SET_MIRROR,
     * 5=SWITCH_CAMERA (toggle from the active lens)
     * For SET_RESOLUTION / SET_MIRROR, [out] holds `[width, height, mirrored]` (0/1).
     */
    external fun takeCameraCommand(handle: Long, out: IntArray?): Int

    /** Last SessionError code (e.g. PUBLIC_KEY_CHANGED), or empty. */
    external fun lastSessionError(handle: Long): String

    /** Pending Rust-owned ABR directive: [id, kind, height, bitrate, epoch]. */
    external fun getEncoderDirective(handle: Long): LongArray?

    external fun ackEncoderDirective(handle: Long, directiveId: Long, actualHeight: Int): Int

    external fun nackEncoderDirective(handle: Long, directiveId: Long): Int

    /** User preferred height for ABR decisions (480, 720, or 1080). */
    external fun setPreferredHeight(handle: Long, height: Int): Int

    /** Allocate a fresh Rust-owned epoch before camera/encoder discontinuity. */
    external fun beginStreamReconfiguration(handle: Long): Int

    external fun cancelStreamReconfiguration(handle: Long, streamEpoch: Int): Int

    /** Host successfully applied an encode height outside an ABR directive. */
    external fun reportEncoderHeight(handle: Long, height: Int, streamEpoch: Int): Int

    external fun bitrateInitialForHeight(height: Int): Int

    external fun bitrateClampForHeight(bitrateBps: Int, height: Int): Int

    /** Thermal hold blocks ABR upshift while overheating (MEDIA-010). */
    external fun setThermalHold(handle: Long, hold: Boolean): Int

    /**
     * Extract SPS/PPS from Annex-B or AVCC codec-config bytes.
     * @return `[sps, pps]` or null when extraction fails.
     */
    external fun extractSpsPps(data: ByteArray): Array<ByteArray>?

    /** Canonical Rust validation for Android NSD TXT bytes. */
    external fun parseDiscoveryTxt(keys: Array<String>, values: Array<ByteArray>): Array<String>?

    external fun createDiscoveryBrowser(): Long

    external fun destroyDiscoveryBrowser(handle: Long)

    external fun pollDiscoveryBrowser(handle: Long, timeoutMs: Int): Int

    external fun getDiscoveryCount(handle: Long): Int

    external fun getDiscoveredReceiver(handle: Long, index: Int): DiscoveredReceiver?

    external fun attachTrustedStore(handle: Long, path: String): Int

    external fun getConnectedReceiverId(handle: Long): String

    external fun getConnectedReceiverDisplayName(handle: Long): String

    external fun loadTrustedStore(path: String): Long

    external fun destroyTrustedStore(handle: Long)

    external fun getTrustedDeviceCount(handle: Long): Int

    external fun getTrustedDevice(handle: Long, index: Int): TrustedDevice?

    external fun removeTrustedDevice(handle: Long, deviceId: String): Int

    /** Clear all trusted devices. Returns count removed (≥0). */
    external fun clearTrustedDevices(handle: Long): Int

    external fun saveTrustedStore(handle: Long): Int

    /** Materialize an Ed25519 identity from a Keystore-protected secret. */
    external fun loadIdentityFromSecret(secret: ByteArray, defaultName: String): Long

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

    /** Export diagnostics with session snapshot (REQ-PICOO-PRIVACY-003). */
    external fun exportDiagnosticsToPathWithSession(
        trustedStorePath: String,
        platform: String,
        appVersion: String,
        role: String,
        status: String,
        accessUnits: Long,
        /** Role-path packets (sender egress datagrams). */
        packets: Long,
        packetsDroppedUnpaired: Long,
        peerHost: String?,
        outPath: String,
    ): Int

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
        /** TXT `platform` (`windows` / `macos`). */
        val platform: String,
        /** TXT `pairing_state` (`open` / `paired_only`); null if absent. */
        val pairingState: String? = null,
    )

    data class TrustedDevice(
        val deviceId: String,
        val deviceName: String,
        val certificateFingerprint: String,
        val pairedAtMs: Long,
        val lastConnectedAtMs: Long,
    )

    data class EncoderDirective(
        val id: Long,
        val kind: Int,
        val targetHeight: Int,
        val targetBitrateBps: Int,
        val streamEpoch: Int,
    )

    data class SenderSnapshot(
        val status: Int,
        val currentBitrateBps: Int,
        val activeHeight: Int,
        val receiverMaxHeight: Int,
        val streamEpoch: Int,
        val reconnectAttempt: Int,
        val reconnectDelayMs: Long,
    )

    fun readSenderSnapshot(handle: Long): SenderSnapshot {
        val values = getSenderSnapshot(handle)
        return SenderSnapshot(
            status = values.getOrElse(0) { STATUS_DISCONNECTED.toLong() }.toInt(),
            currentBitrateBps = values.getOrElse(1) { 0 }.toInt(),
            activeHeight = values.getOrElse(2) { 0 }.toInt(),
            receiverMaxHeight = values.getOrElse(3) { 0 }.toInt(),
            streamEpoch = values.getOrElse(4) { 0 }.toInt(),
            reconnectAttempt = values.getOrElse(5) { 0 }.toInt(),
            reconnectDelayMs = values.getOrElse(6) { 0 },
        )
    }

    fun readEncoderDirective(handle: Long): EncoderDirective? {
        val values = getEncoderDirective(handle) ?: return null
        if (values.size < 5) return null
        return EncoderDirective(
            id = values[0],
            kind = values[1].toInt(),
            targetHeight = values[2].toInt(),
            targetBitrateBps = values[3].toInt(),
            streamEpoch = values[4].toInt(),
        )
    }

    fun readSenderStats(handle: Long): SenderStats {
        check(isAvailable) { "native unavailable: $loadError" }
        val values = getSenderStats(handle)
        return SenderStats(
            accessUnits = values.getOrElse(0) { 0 },
            packets = values.getOrElse(1) { 0 },
            bytes = values.getOrElse(2) { 0 },
            sentDatagrams = values.getOrElse(3) { 0 },
            pendingPackets = values.getOrElse(4) { 0 },
        )
    }

    fun statusLabel(status: Int): String = SenderStatusCodes.label(status)
}
