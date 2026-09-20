package com.picoo.camera.media

/** Input geometry for an explicit source request — REQ-PICOO-MEDIA-050. */
object CaptureSizeSelector {
    data class Dim(val width: Int, val height: Int)

    /** Candidates must already satisfy the same camera's requested fixed rate. */
    fun select(
        available: List<Dim>,
        target: Dim,
        portraitCrop: Boolean = false,
    ): Dim {
        require(target.width > 0 && target.height > 0) { "Invalid source dimensions" }
        return available.filter { candidate ->
            val width = if (portraitCrop) candidate.height else candidate.width
            val height = if (portraitCrop) candidate.width else candidate.height
            width >= target.width && height >= target.height
        }.minWithOrNull(
            compareBy<Dim> { it.width.toLong() * it.height.toLong() }
                .thenBy { it.width }.thenBy { it.height },
        ) ?: error("Camera cannot provide the requested image without upscaling")
    }
}
