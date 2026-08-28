package com.picoo.camera.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ExposurePreviewTest {
    @Test
    fun autoHasNoOverlay() {
        assertEquals(0f, ExposurePreview.overlayAlpha(0), 0.001f)
    }

    @Test
    fun brighterLevelsIncreaseWhiteAlpha() {
        assertTrue(ExposurePreview.overlayAlpha(2) > ExposurePreview.overlayAlpha(1))
        assertTrue(ExposurePreview.overlayAlpha(1) > 0f)
    }

    @Test
    fun darkerLevelsIncreaseBlackAlpha() {
        assertTrue(ExposurePreview.overlayAlpha(-2) > ExposurePreview.overlayAlpha(-1))
        assertTrue(ExposurePreview.overlayAlpha(-1) > 0f)
    }
}
