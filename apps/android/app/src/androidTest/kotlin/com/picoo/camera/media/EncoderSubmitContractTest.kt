package com.picoo.camera.media

import com.picoo.camera.jni.PicooNative
import org.junit.Assert.assertNull
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
            assertNull(PicooNative.readSenderSnapshot(sender).receiverSourceFormats)
            assertNull(PicooNative.readSenderSnapshot(sender).lastCommittedSourceFormat)
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


    @Test
    fun sourceRequestAdmitsExplicitCodecAndFrameRate() {
        assertTrue(PicooNative.ensureLoaded())
        val identity = PicooNative.loadIdentityFromSecret(ByteArray(32) { 0x38 }, "format-test")
        assertTrue(identity != 0L)
        var sender = 0L
        try {
            sender = PicooNative.createSender(identity)
            assertTrue(sender != 0L)
            assertEquals(0, PicooNative.beginStreamReconfiguration(sender, 1080, 0, 60))
            assertEquals(0, PicooNative.beginStreamReconfiguration(sender, 1080, 2, 120))
            val epoch = PicooNative.beginStreamReconfiguration(sender, 1080, 2, 60)
            assertTrue(epoch > 1)
            // The effect getter must not instruct the owner to repeat its local apply.
            assertEquals(null, PicooNative.readEncoderDirective(sender))
            assertTrue(PicooNative.encoderTransactionId(sender, epoch) > 0)
        } finally {
            if (sender != 0L) PicooNative.destroySender(sender)
            PicooNative.destroyIdentity(identity)
        }
    }

}
