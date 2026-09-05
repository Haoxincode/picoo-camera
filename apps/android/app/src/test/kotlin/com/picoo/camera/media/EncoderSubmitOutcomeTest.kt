package com.picoo.camera.media

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class EncoderSubmitOutcomeTest {
    @Test
    fun negativeNativeCodesRemainFailuresInsteadOfBitFlags() {
        assertEquals(EncoderSubmitOutcome.Failure(-1), EncoderSubmitOutcome.fromNative(-1))
        assertEquals(EncoderSubmitOutcome.Failure(-2), EncoderSubmitOutcome.fromNative(-2))
    }

    @Test
    fun zeroIsAValidSuccessWithoutSideEffects() {
        val outcome = EncoderSubmitOutcome.fromNative(0) as EncoderSubmitOutcome.Success
        assertFalse(outcome.encoderAccepted)
        assertFalse(outcome.streamConfigured)
        assertFalse(outcome.keyframeRequested)
    }

    @Test
    fun legalFlagCombinationIsDecodedOnlyOnTheSuccessPath() {
        val outcome = EncoderSubmitOutcome.fromNative(0b111) as EncoderSubmitOutcome.Success
        assertTrue(outcome.encoderAccepted)
        assertTrue(outcome.streamConfigured)
        assertTrue(outcome.keyframeRequested)
    }
}
