package com.picoo.camera.media

import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

/** REQ-PICOO-MEDIA-050: reject unavailable source geometry without changing the request. */
class CaptureSizeSelectorTest {
    private val hd = CaptureSizeSelector.Dim(1280, 720)
    private val fullHd = CaptureSizeSelector.Dim(1920, 1080)

    @Test
    fun selectsSmallestSufficientInputForEachExplicitTarget() {
        val available = listOf(CaptureSizeSelector.Dim(640, 480), hd, fullHd)
        assertEquals(hd, CaptureSizeSelector.select(available, hd))
        assertEquals(fullHd, CaptureSizeSelector.select(available, fullHd))
    }

    @Test
    fun missingOrUndersizedCandidatesDoNotInventSupportOrDowngrade() {
        for (available in listOf(emptyList(), listOf(hd), listOf(CaptureSizeSelector.Dim(1800, 1000)))) {
            assertThrows(IllegalStateException::class.java) {
                CaptureSizeSelector.select(available, fullHd)
            }
        }
    }

    @Test
    fun portraitCropNeedsEnoughPixelsAfterRotation() {
        val available = listOf(fullHd, CaptureSizeSelector.Dim(3840, 2160), CaptureSizeSelector.Dim(2560, 1920))
        assertEquals(CaptureSizeSelector.Dim(2560, 1920), CaptureSizeSelector.select(available, fullHd, portraitCrop = true))
        assertThrows(IllegalStateException::class.java) {
            CaptureSizeSelector.select(listOf(hd, fullHd), fullHd, portraitCrop = true)
        }
    }
}
