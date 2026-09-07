package com.picoo.camera.media

import org.junit.Assert.assertEquals
import org.junit.Test

class VideoSourceFormatTest {
    @Test fun defaultIsAnExplicit1080p60AvcRequest() {
        assertEquals(VideoSourceFormat(NativeVideoCodec.Avc, StreamResolution.P1080, 60), VideoSourceFormat.Default)
    }

    @Test fun productChoicesCoverOnlyTheEightFormalCombinations() {
        assertEquals(setOf(
            "H.264 · 1080p · 60 fps", "H.264 · 1080p · 30 fps",
            "H.264 · 720p · 60 fps", "H.264 · 720p · 30 fps",
            "HEVC · 1080p · 60 fps", "HEVC · 1080p · 30 fps",
            "HEVC · 720p · 60 fps", "HEVC · 720p · 30 fps",
        ), VideoSourceFormat.ProductFormats.map { it.label }.toSet())
        assertEquals(8, VideoSourceFormat.ProductFormats.size)
    }
}
