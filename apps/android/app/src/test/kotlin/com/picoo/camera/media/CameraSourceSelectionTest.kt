package com.picoo.camera.media

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class CameraSourceSelectionTest {
    private val avc1080p30 = VideoSourceFormat(NativeVideoCodec.Avc, StreamResolution.P1080, 30)
    private val avc720p60 = VideoSourceFormat(NativeVideoCodec.Avc, StreamResolution.P720, 60)
    private val hevc1080p60 = VideoSourceFormat(NativeVideoCodec.Hevc, StreamResolution.P1080, 60)

    @Test fun initialMediaKeepsAvailablePreferenceAndAdaptsUnavailableDefaultAfterConnection() {
        assertEquals(avc1080p30, CameraSourceSelection.initial(listOf(avc1080p30), VideoSourceFormat.Default))
        assertEquals(avc720p60, CameraSourceSelection.initial(listOf(avc720p60, VideoSourceFormat.Default), avc720p60))
        assertNull(CameraSourceSelection.initial(emptyList(), VideoSourceFormat.Default))
    }

    @Test fun lensCeilingPreservesCodecAndPrioritizesResolutionWithoutInventing1080p60() {
        assertEquals(avc1080p30, CameraSourceSelection.select(
            listOf(avc720p60, hevc1080p60, avc1080p30), NativeVideoCodec.Avc))
        assertEquals(hevc1080p60, CameraSourceSelection.select(
            listOf(avc720p60, hevc1080p60, avc1080p30), NativeVideoCodec.Hevc))
    }

    @Test fun returningToCapableLensRestoresCeilingAndNoIntersectionRemainsExplicit() {
        assertEquals(VideoSourceFormat.Default, CameraSourceSelection.select(
            listOf(avc1080p30, VideoSourceFormat.Default), NativeVideoCodec.Avc))
        assertEquals(avc720p60, CameraSourceSelection.select(listOf(avc720p60), NativeVideoCodec.Hevc))
        assertNull(CameraSourceSelection.select(emptyList(), NativeVideoCodec.Avc))
    }
}
