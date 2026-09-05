package com.picoo.camera.media

/** Immutable H.264 AU detached from MediaCodec before its output buffer is released. */
internal data class EncodedAccessUnitHandoff(
    val data: ByteArray,
    val isKeyFrame: Boolean,
    val presentationTimeUs: Long,
    val encodedAtUs: Long,
    val streamEpoch: Int,
    val encoderGeneration: Long,
    val encoderWidth: Int,
    val encoderHeight: Int,
    val enqueuedAtNanos: Long,
)

internal data class EncodedAccessUnitWork(
    val accessUnit: EncodedAccessUnitHandoff?,
    val recoveryRequired: Boolean,
)

internal data class EncodedAccessUnitOffer(
    val accepted: Boolean,
    val scheduleWorker: Boolean,
)

/**
 * Bounded, GOP-aware handoff from the MediaCodec callback to one Sender media worker.
 *
 * Capacity, bytes and monotonic frame age are independent bounds. Once a reference chain is
 * discarded, deltas stay out of the queue until a new IDR arrives.
 */
internal class EncodedAccessUnitBuffer(
    private val capacity: Int = 12,
    private val maxBytes: Int = 16 * 1024 * 1024,
    private val maxAgeNanos: Long = 250_000_000L,
) {
    private val events = ArrayDeque<EncodedAccessUnitHandoff>()
    private var queuedBytes = 0
    private var waitingForKeyFrame = false
    private var recoveryPending = false
    private var workerScheduled = false
    private var closed = false

    init {
        require(capacity >= 2)
        require(maxBytes > 0)
        require(maxAgeNanos > 0)
    }

    @Synchronized
    fun offer(event: EncodedAccessUnitHandoff, nowNanos: Long): EncodedAccessUnitOffer {
        if (closed) return EncodedAccessUnitOffer(accepted = false, scheduleWorker = false)
        expireIfNeeded(nowNanos)
        if (waitingForKeyFrame && !event.isKeyFrame) {
            return schedule(accepted = false)
        }

        if (events.size >= capacity || queuedBytes + event.data.size > maxBytes) {
            discardReferenceChain()
        }
        val accepted = if (event.data.size <= maxBytes && (!waitingForKeyFrame || event.isKeyFrame)) {
            events.addLast(event)
            queuedBytes += event.data.size
            if (event.isKeyFrame) {
                waitingForKeyFrame = false
                recoveryPending = false
            }
            true
        } else {
            recoveryPending = true
            waitingForKeyFrame = true
            false
        }
        return schedule(accepted)
    }

    /** Called repeatedly by the single worker until it returns null. */
    @Synchronized
    fun take(nowNanos: Long): EncodedAccessUnitWork? {
        expireIfNeeded(nowNanos)
        val recovery = recoveryPending
        recoveryPending = false
        val event = events.removeFirstOrNull()
        if (event != null) queuedBytes -= event.data.size
        if (event == null && !recovery) {
            workerScheduled = false
            return null
        }
        return EncodedAccessUnitWork(event, recovery)
    }

    @Synchronized
    fun close() {
        closed = true
        events.clear()
        queuedBytes = 0
        recoveryPending = false
    }

    @Synchronized
    internal fun queuedEventCount(): Int = events.size

    @Synchronized
    internal fun queuedByteCount(): Int = queuedBytes

    private fun expireIfNeeded(nowNanos: Long) {
        val oldest = events.firstOrNull() ?: return
        if (nowNanos - oldest.enqueuedAtNanos >= maxAgeNanos) {
            discardReferenceChain()
        }
    }

    private fun discardReferenceChain() {
        events.clear()
        queuedBytes = 0
        waitingForKeyFrame = true
        recoveryPending = true
    }

    private fun schedule(accepted: Boolean): EncodedAccessUnitOffer {
        val needsWorker = (accepted || recoveryPending) && !workerScheduled
        if (needsWorker) workerScheduled = true
        return EncodedAccessUnitOffer(accepted, needsWorker)
    }
}
