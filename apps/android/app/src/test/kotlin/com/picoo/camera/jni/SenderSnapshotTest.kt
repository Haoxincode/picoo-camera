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
        assertNull(PicooNative.SenderSnapshot.fromNative(LongArray(11)).receiverSourceFormats)
        assertNull(PicooNative.SenderSnapshot.fromNative(LongArray(11)).lastCommittedSourceFormat)
        val known = LongArray(11).also { it[7] = 1 }
        assertEquals(emptyList<VideoSourceFormat>(), PicooNative.SenderSnapshot.fromNative(known).receiverSourceFormats)
    }

    @Test fun mixedCandidatesPreserveCodecResolutionAndFrameRate() {
        val values = longArrayOf(5, 24_000_000, 1080, 1080, 7, 2, 500, 1, 2, 720, 60, 1, 720, 30, 2, 1080, 60)
        val snapshot = PicooNative.SenderSnapshot.fromNative(values)
        assertEquals(7, snapshot.streamEpoch)
        assertEquals(VideoSourceFormat(NativeVideoCodec.Hevc, StreamResolution.P720, 60), snapshot.lastCommittedSourceFormat)
        assertEquals(listOf(
            VideoSourceFormat(NativeVideoCodec.Avc, StreamResolution.P720, 30),
            VideoSourceFormat(NativeVideoCodec.Hevc, StreamResolution.P1080, 60),
        ), snapshot.receiverSourceFormats)
    }

    @Test fun malformedOrUnsupportedNativeCandidatesFailAtTheBoundary() {
        val malformed = listOf(
            LongArray(8), LongArray(10), LongArray(38),
            LongArray(11).also { it[9] = 720 },
            LongArray(11).also { it[8] = 1; it[9] = 720; it[10] = 24 },
            longArrayOf(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 720, 30),
            longArrayOf(0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 720, 24),
            longArrayOf(0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 99, 1080, 60),
        )
        malformed.forEach { values ->
            assertThrows(IllegalStateException::class.java) { PicooNative.SenderSnapshot.fromNative(values) }
        }
    }
}
