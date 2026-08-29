package com.picoo.camera

/**
 * Streaming-page display brightness — REQ-PICOO-UI-005 / PUC-005 / PRD FR-CAM-005.
 *
 * Keeps the transmission UI dark and low-brightness without turning the screen off
 * ([android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON] still applies).
 */
object StreamingBrightness {
    /** Dimmed level while Streaming tab is active and the session is live. */
    const val DIMMED: Float = 0.12f
}
