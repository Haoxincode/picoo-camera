package com.picoo.camera.media

/**
 * Pick a Camera2 output size for the negotiated encode target (REQ-PICOO-MEDIA-002).
 *
 * Prefer the closest available size; when 1080p is requested but no near-1080
 * option exists, fall back to the best ≤720p candidate (OEM capability fallback).
 *
 * Uses plain ints so JVM unit tests do not need Robolectric / android.jar.
 */
object CaptureSizeSelector {
    data class Dim(val width: Int, val height: Int)

    data class Choice(val size: Dim, val fellBackFrom1080: Boolean)

    /** Manhattan distance in pixels between two sizes. */
    fun distance(a: Dim, b: Dim): Int =
        kotlin.math.abs(a.width - b.width) + kotlin.math.abs(a.height - b.height)

    /**
     * @param available output sizes from SCALER_STREAM_CONFIGURATION_MAP
     * @param target preferred encode size (typically 1280×720 or 1920×1080)
     */
    fun select(available: List<Dim>, target: Dim): Choice {
        if (available.isEmpty()) {
            return Choice(target, fellBackFrom1080 = false)
        }

        val wants1080 = target.height >= 1080 || target.width >= 1920
        val near1080 = available.filter { it.height >= 1000 || it.width >= 1800 }

        if (wants1080 && near1080.isEmpty()) {
            val capped = available
                .filter { it.height <= 720 || (it.width <= 1280 && it.height <= 800) }
                .ifEmpty { available }
            val best = capped.minBy { distance(it, Dim(1280, 720)) }
            return Choice(best, fellBackFrom1080 = true)
        }

        val pool = if (wants1080 && near1080.isNotEmpty()) near1080 else available
        val best = pool.minBy { distance(it, target) }
        return Choice(best, fellBackFrom1080 = false)
    }

    /** Encode resolution should follow capability fallback (canonical 720p when 1080 unavailable). */
    fun encodeSizeFor(choice: Choice, requested: Dim): Dim =
        if (choice.fellBackFrom1080) Dim(1280, 720) else requested
}
