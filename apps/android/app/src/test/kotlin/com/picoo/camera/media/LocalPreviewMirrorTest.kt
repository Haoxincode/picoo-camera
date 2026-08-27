package com.picoo.camera.media

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** REQ-PICOO-MEDIA-004: local preview mirror defaults by lens facing. */
class LocalPreviewMirrorTest {
    @Test
    fun frontCameraDefaultsMirrored() {
        assertTrue(LocalPreviewMirror.defaultFor(LensFacing.Front))
    }

    @Test
    fun backCameraDefaultsNotMirrored() {
        assertFalse(LocalPreviewMirror.defaultFor(LensFacing.Back))
    }
}
