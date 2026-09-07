package com.picoo.camera.media

import com.picoo.camera.jni.PicooNative
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** Executes the packaged JNI constructors; native errors cannot become acceptance flags. */
class EncoderSubmitContractTest {
    @Test
    fun invalidInputReturnsAnExplicitErrorObject() {
        assertTrue(PicooNative.ensureLoaded())
        val result = PicooNative.submitEncoderAccessUnit(
            handle = 0,
            data = byteArrayOf(),
            keyframe = true,
            ptsUs = 1,
            encodedAtUs = 2,
            streamEpoch = 1,
            encoderGeneration = 1,
            encoderWidth = 1280,
            encoderHeight = 720,
            configureStream = false,
            mirrored = false,
            codec = NativeVideoCodec.Avc.wireValue,
            fps = 30,
            codecConfiguration = null,
        )
        assertEquals(EncoderSubmitOutcome.Error(-2), result)
    }

    @Test
    fun staleGenerationReturnsRejectionWithoutConfigurationSideEffects() {
        assertTrue(PicooNative.ensureLoaded())
        val identity = PicooNative.loadIdentityFromSecret(ByteArray(32) { 0x37 }, "submission-test")
        assertTrue(identity != 0L)
        var sender = 0L
        try {
            sender = PicooNative.createSender(identity)
            assertTrue(sender != 0L)
            val result = PicooNative.submitEncoderAccessUnit(
                handle = sender,
                data = byteArrayOf(0, 0, 0, 1, 0x65, 0x80.toByte()),
                keyframe = true,
                ptsUs = 1,
                encodedAtUs = 2,
                streamEpoch = Int.MAX_VALUE,
                encoderGeneration = 1,
                encoderWidth = 1280,
                encoderHeight = 720,
                configureStream = false,
                mirrored = false,
                codec = NativeVideoCodec.Avc.wireValue,
                fps = 30,
                codecConfiguration = null,
            )
            assertEquals(EncoderSubmitOutcome.Rejected(false), result)
        } finally {
            if (sender != 0L) PicooNative.destroySender(sender)
            PicooNative.destroyIdentity(identity)
        }
    }

}
