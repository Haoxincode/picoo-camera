package com.picoo.camera.media

/** REQ-PICOO-MEDIA-013 — encode the camera buffer's rotation relative to the display. */
object StreamOrientation {
    fun relativeRotationDegrees(
        sensorOrientationDegrees: Int,
        displayRotationDegrees: Int,
        frontFacing: Boolean,
    ): Int {
        val rotation = if (frontFacing) {
            sensorOrientationDegrees + displayRotationDegrees
        } else {
            sensorOrientationDegrees - displayRotationDegrees
        }
        return ((rotation % 360) + 360) % 360
    }
}
