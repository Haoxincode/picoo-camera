package com.picoo.camera.jni

import com.picoo.camera.media.NativeVideoCodec
import com.picoo.camera.media.StreamResolution
import com.picoo.camera.media.VideoSourceFormat
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Test

/** REQ-PICOO-MEDIA-054: the array boundary cannot invent combinations or treat unknown as empty. */
class SenderSnapshotTest {
    @Test fun unknownAndKnownEmptyEvidenceRemainDifferent() {
        assertNull(PicooNative.SenderSnapshot.fromNative(LongArray(8)).receiverSourceFormats)
        val known = LongArray(8).also { it[7] = 1 }
        assertEquals(emptyList<VideoSourceFormat>(), PicooNative.SenderSnapshot.fromNative(known).receiverSourceFormats)
    }

    @Test fun mixedCandidatesPreserveCodecResolutionAndFrameRate() {
        val values = longArrayOf(5, 24_000_000, 1080, 1080, 7, 2, 500, 1, 1, 720, 30, 2, 1080, 60)
        val snapshot = PicooNative.SenderSnapshot.fromNative(values)
        assertEquals(7, snapshot.streamEpoch)
        assertEquals(listOf(
            VideoSourceFormat(NativeVideoCodec.Avc, StreamResolution.P720, 30),
            VideoSourceFormat(NativeVideoCodec.Hevc, StreamResolution.P1080, 60),
        ), snapshot.receiverSourceFormats)
    }

    @Test fun malformedOrUnsupportedNativeCandidatesFailAtTheBoundary() {
        val malformed = listOf(
            LongArray(7), LongArray(10), LongArray(35),
            longArrayOf(0, 0, 0, 0, 0, 0, 0, 0, 1, 720, 30),
            longArrayOf(0, 0, 0, 0, 0, 0, 0, 1, 1, 720, 24),
            longArrayOf(0, 0, 0, 0, 0, 0, 0, 1, 99, 1080, 60),
        )
        malformed.forEach { values ->
            assertThrows(IllegalStateException::class.java) { PicooNative.SenderSnapshot.fromNative(values) }
        }
    }
}
