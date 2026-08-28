package com.picoo.camera.ui

import com.picoo.camera.media.StreamResolution
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ResolutionSheetOptionsTest {
    @Test
    fun coversThreeTiersInPrototypeOrder() {
        assertEquals(
            listOf(StreamResolution.P1080, StreamResolution.P720, StreamResolution.P480),
            ResolutionSheetOptions.all.map { it.resolution },
        )
    }

    @Test
    fun titlesMatchPrototypeSheet() {
        val titles = ResolutionSheetOptions.all.map { it.title }
        assertTrue(titles[0].startsWith("1080p30"))
        assertTrue(titles[1].startsWith("720p30"))
        assertTrue(titles[2].startsWith("480p30"))
    }
}
