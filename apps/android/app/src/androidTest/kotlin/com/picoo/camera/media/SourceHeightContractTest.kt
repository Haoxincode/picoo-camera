package com.picoo.camera.media

import com.picoo.camera.jni.PicooNative
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** REQ-PICOO-MEDIA-028: verify the packaged JNI boundary on Android. */
class SourceHeightContractTest {
    @Test
    fun unsupportedSourceHeightsNeverBecomeAnotherBitrateProfile() {
        assertTrue(PicooNative.ensureLoaded())
        for (height in listOf(-1, 0, 480, 719, 721, 1079, 1081, 2160, Int.MAX_VALUE)) {
            assertEquals(0, PicooNative.bitrateInitialForHeight(height))
            assertEquals(0, PicooNative.bitrateClampForHeight(4_000_000, height))
        }
        assertEquals(3_000_000, PicooNative.bitrateInitialForHeight(720))
        assertEquals(6_000_000, PicooNative.bitrateInitialForHeight(1080))
    }
}
