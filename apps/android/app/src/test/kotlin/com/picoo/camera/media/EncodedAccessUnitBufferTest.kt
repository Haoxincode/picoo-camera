package com.picoo.camera.media

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class EncodedAccessUnitBufferTest {
    private fun accessUnit(
        id: Int,
        keyFrame: Boolean = false,
        bytes: Int = 4,
        enqueuedAtNanos: Long = 0,
    ) = EncodedAccessUnitHandoff(
        data = ByteArray(bytes) { id.toByte() },
        isKeyFrame = keyFrame,
        presentationTimeUs = id.toLong(),
        encodedAtUs = id.toLong(),
        streamEpoch = 1,
        encoderGeneration = 1,
        encoderWidth = 1280,
        encoderHeight = 720,
        enqueuedAtNanos = enqueuedAtNanos,
    )

    @Test
    fun capacityOverflowDropsChainUntilKeyFrame() {
        val buffer = EncodedAccessUnitBuffer(capacity = 2, maxBytes = 100, maxAgeNanos = 100)
        assertTrue(buffer.offer(accessUnit(1), 0).accepted)
        assertTrue(buffer.offer(accessUnit(2), 0).accepted)
        assertFalse(buffer.offer(accessUnit(3), 0).accepted)

        val recovery = buffer.take(0)!!
        assertTrue(recovery.recoveryRequired)
        assertNull(recovery.accessUnit)
        assertFalse(buffer.offer(accessUnit(4), 0).accepted)
        assertTrue(buffer.offer(accessUnit(5, keyFrame = true), 0).accepted)
        assertTrue(buffer.take(0)!!.accessUnit!!.isKeyFrame)
    }

    @Test
    fun byteAndAgeBoundsDiscardWholeReferenceChain() {
        val buffer = EncodedAccessUnitBuffer(capacity = 4, maxBytes = 8, maxAgeNanos = 10)
        buffer.offer(accessUnit(1, bytes = 5), 0)
        assertFalse(buffer.offer(accessUnit(2, bytes = 5), 0).accepted)
        assertEquals(0, buffer.queuedByteCount())
        assertTrue(buffer.take(0)!!.recoveryRequired)

        buffer.offer(accessUnit(3, keyFrame = true, enqueuedAtNanos = 0), 0)
        assertTrue(buffer.take(10)!!.recoveryRequired)
        assertEquals(0, buffer.queuedEventCount())
    }

    @Test
    fun keyFrameAcceptedAtOverflowRepairsChainWithoutRedundantRecoveryRequest() {
        val buffer = EncodedAccessUnitBuffer(capacity = 2, maxBytes = 100, maxAgeNanos = 100)
        buffer.offer(accessUnit(1), 0)
        buffer.offer(accessUnit(2), 0)

        assertTrue(buffer.offer(accessUnit(3, keyFrame = true), 0).accepted)
        val work = buffer.take(0)!!
        assertFalse(work.recoveryRequired)
        assertTrue(work.accessUnit!!.isKeyFrame)
        assertNull(buffer.take(0))
    }
}
