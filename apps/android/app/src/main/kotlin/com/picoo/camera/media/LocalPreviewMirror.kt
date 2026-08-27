package com.picoo.camera.media

/**
 * Default local preview mirror by lens facing (REQ-PICOO-MEDIA-004 / FR-CAM-003).
 *
 * Front camera: mirror on (selfie convention). Back camera: mirror off.
 * Independent of remote [StreamConfig.mirrored].
 */
object LocalPreviewMirror {
    fun defaultFor(facing: LensFacing): Boolean =
        when (facing) {
            LensFacing.Front -> true
            LensFacing.Back -> false
        }
}
