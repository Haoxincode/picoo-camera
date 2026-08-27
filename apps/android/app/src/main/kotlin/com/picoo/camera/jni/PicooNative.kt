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

    /** [accessUnits, packets, bytes, pendingPackets] */
    external fun getSenderStats(handle: Long): LongArray

    data class SenderStats(
        val accessUnits: Long,
        val packets: Long,
        val bytes: Long,
        val pendingPackets: Long,
    )

    fun readSenderStats(handle: Long): SenderStats {
        val values = getSenderStats(handle)
        return SenderStats(
            accessUnits = values.getOrElse(0) { 0 },
            packets = values.getOrElse(1) { 0 },
            bytes = values.getOrElse(2) { 0 },
            pendingPackets = values.getOrElse(3) { 0 },
        )
    }
}
