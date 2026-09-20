package com.picoo.camera.ui

import com.picoo.camera.jni.PicooNative
import com.picoo.camera.media.NativeVideoCodec
import com.picoo.camera.media.StreamResolution
import com.picoo.camera.media.VideoSourceFormat
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** MEDIA-057: offline preparation is not fresh remote capability evidence. */
class SourceAvailabilityTest {
    @Test fun connectedSelectionWaitsForEvidenceAndPreservesUserIntent() {
        val avc = VideoSourceFormat(NativeVideoCodec.Avc, StreamResolution.P720, 30)
        val hevc = VideoSourceFormat(NativeVideoCodec.Hevc, StreamResolution.P1080, 60)
        val state = SenderHomeState()
        state.localSourceFormats = listOf(avc, hevc)
        state.preferredSourceFormat = avc
        state.committedSourceFormat = avc
        assertEquals(listOf(avc, hevc), state.availableSourceFormats)
        state.senderStatus = PicooNative.STATUS_STREAMING
        assertNull(state.availableSourceFormats)
        state.receiverSourceFormats = listOf(hevc, VideoSourceFormat.Default)
        assertEquals(listOf(hevc), state.availableSourceFormats)
        assertEquals(avc, state.preferredSourceFormat)
        assertEquals(avc, state.committedSourceFormat)
        state.receiverSourceFormats = emptyList()
        assertEquals(emptyList<VideoSourceFormat>(), state.availableSourceFormats)
        state.senderStatus = PicooNative.STATUS_RECONNECTING
        state.receiverSourceFormats = null
        assertNull(state.availableSourceFormats)
    }
}
