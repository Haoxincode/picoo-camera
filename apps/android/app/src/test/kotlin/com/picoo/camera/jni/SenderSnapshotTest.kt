package com.picoo.camera.jni

import com.picoo.camera.media.NativeVideoCodec
import com.picoo.camera.media.StreamResolution
import com.picoo.camera.media.VideoSourceFormat
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Test

/** MEDIA-054/055: preserve unknown evidence and actual facts without height-only guesses. */
class SenderSnapshotTest {
    @Test fun unknownAndKnownEmptyEvidenceRemainDifferent() {
        val unknown = PicooNative.SenderSnapshot.fromNative(LongArray(9))
        assertNull(unknown.receiverSourceFormats)
        assertNull(unknown.lastCommittedSourceFormat)
        val known = LongArray(9).also { it[5] = 1 }
        assertEquals(emptyList<VideoSourceFormat>(), PicooNative.SenderSnapshot.fromNative(known).receiverSourceFormats)
    }

    @Test fun mixedCandidatesPreserveCodecResolutionAndFrameRate() {
        val values = longArrayOf(5, 24_000_000, 7, 2, 500, 1, 2, 720, 60, 1, 720, 30, 2, 1080, 60)
        val snapshot = PicooNative.SenderSnapshot.fromNative(values)
        assertEquals(7, snapshot.streamEpoch)
        assertEquals(2, snapshot.reconnectAttempt)
        assertEquals(500L, snapshot.reconnectDelayMs)
        assertEquals(VideoSourceFormat(NativeVideoCodec.Hevc, StreamResolution.P720, 60), snapshot.lastCommittedSourceFormat)
        assertEquals(listOf(
            VideoSourceFormat(NativeVideoCodec.Avc, StreamResolution.P720, 30),
            VideoSourceFormat(NativeVideoCodec.Hevc, StreamResolution.P1080, 60),
        ), snapshot.receiverSourceFormats)
    }

    @Test fun malformedOrUnsupportedNativeCandidatesFailAtTheBoundary() {
        val malformed = listOf(
            LongArray(8), LongArray(11), LongArray(36),
            LongArray(9).also { it[7] = 720 },
            LongArray(9).also { it[6] = 1; it[7] = 720; it[8] = 24 },
            longArrayOf(0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 720, 30),
            longArrayOf(0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 720, 24),
            longArrayOf(0, 0, 0, 0, 0, 1, 0, 0, 0, 99, 1080, 60),
        )
        malformed.forEach { values ->
            assertThrows(IllegalStateException::class.java) { PicooNative.SenderSnapshot.fromNative(values) }
        }
    }
}
