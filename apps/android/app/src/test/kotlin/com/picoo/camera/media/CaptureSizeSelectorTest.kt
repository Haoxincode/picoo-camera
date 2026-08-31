package com.picoo.camera.media

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** REQ-PICOO-MEDIA-002: capture size selection + 1080→720 capability fallback. */
class CaptureSizeSelectorTest {
    @Test
    fun picksClosest720pWhenAvailable() {
        val available = listOf(
            CaptureSizeSelector.Dim(640, 480),
            CaptureSizeSelector.Dim(1280, 720),
            CaptureSizeSelector.Dim(1920, 1080),
        )
        val choice = CaptureSizeSelector.select(available, CaptureSizeSelector.Dim(1280, 720))
        assertEquals(1280, choice.size.width)
        assertEquals(720, choice.size.height)
        assertFalse(choice.fellBackFrom1080)
    }

    @Test
    fun picksClosest1080pWhenAvailable() {
        val available = listOf(
            CaptureSizeSelector.Dim(1280, 720),
            CaptureSizeSelector.Dim(1920, 1080),
            CaptureSizeSelector.Dim(2560, 1440),
        )
        val choice = CaptureSizeSelector.select(available, CaptureSizeSelector.Dim(1920, 1080))
        assertEquals(1920, choice.size.width)
        assertEquals(1080, choice.size.height)
        assertFalse(choice.fellBackFrom1080)
    }

    @Test
    fun fallsBackTo720pWhen1080Unavailable() {
        val available = listOf(
            CaptureSizeSelector.Dim(640, 480),
            CaptureSizeSelector.Dim(1280, 720),
            CaptureSizeSelector.Dim(800, 600),
        )
        val choice = CaptureSizeSelector.select(available, CaptureSizeSelector.Dim(1920, 1080))
        assertTrue(choice.fellBackFrom1080)
        assertEquals(1280, choice.size.width)
        assertEquals(720, choice.size.height)
        assertEquals(
            CaptureSizeSelector.Dim(1280, 720),
            CaptureSizeSelector.encodeSizeFor(choice, CaptureSizeSelector.Dim(1920, 1080)),
        )
    }

    @Test
    fun emptyListKeepsTarget() {
        val choice = CaptureSizeSelector.select(emptyList(), CaptureSizeSelector.Dim(1920, 1080))
        assertEquals(1920, choice.size.width)
        assertEquals(1080, choice.size.height)
        assertFalse(choice.fellBackFrom1080)
    }

    @Test
    fun portrait1080ChoosesSmallestSourceWhoseShortEdgeAvoidsUpscaling() {
        val available = listOf(
            CaptureSizeSelector.Dim(1920, 1080),
            CaptureSizeSelector.Dim(3840, 2160),
            CaptureSizeSelector.Dim(2560, 1920),
            CaptureSizeSelector.Dim(4000, 3000),
        )

        val choice = CaptureSizeSelector.select(
            available,
            CaptureSizeSelector.Dim(1920, 1080),
            portraitCrop = true,
        )

        assertEquals(CaptureSizeSelector.Dim(2560, 1920), choice.size)
        assertFalse(choice.fellBackFrom1080)
    }

    @Test
    fun portraitFallsBackToClosestSourceWhenNoShortEdgeIsWideEnough() {
        val available = listOf(
            CaptureSizeSelector.Dim(1280, 720),
            CaptureSizeSelector.Dim(1920, 1080),
        )

        val choice = CaptureSizeSelector.select(
            available,
            CaptureSizeSelector.Dim(1920, 1080),
            portraitCrop = true,
        )

        assertEquals(CaptureSizeSelector.Dim(1920, 1080), choice.size)
        assertFalse(choice.fellBackFrom1080)
    }
}
