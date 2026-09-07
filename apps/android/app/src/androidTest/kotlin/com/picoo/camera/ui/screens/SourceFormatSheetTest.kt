package com.picoo.camera.ui.screens

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import com.picoo.camera.media.NativeVideoCodec
import com.picoo.camera.media.StreamResolution
import com.picoo.camera.media.VideoSourceFormat
import com.picoo.camera.ui.theme.PicooCameraTheme
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test

/** MEDIA-057: select one actual candidate, never synthesize codec/fps combinations. */
class SourceFormatSheetTest {
    @get:Rule val compose = createComposeRule()

    @Test fun selectionReturnsTheCompleteAdmittedFormat() {
        val avc = VideoSourceFormat(NativeVideoCodec.Avc, StreamResolution.P720, 30)
        val hevc = VideoSourceFormat(NativeVideoCodec.Hevc, StreamResolution.P1080, 60)
        var selected: VideoSourceFormat? = null
        compose.setContent {
            PicooCameraTheme {
                SourceFormatSheet(avc, listOf(avc, hevc), {}, { selected = it })
            }
        }
        compose.onNodeWithText("H.264 · 1080p · 60 fps").assertDoesNotExist()
        compose.onNodeWithText(hevc.label).performScrollTo().performClick()
        compose.runOnIdle { assertEquals(hevc, selected) }
    }

    @Test fun unknownAndEmptyPreparationDoNotOfferAGuessedDefault() {
        var candidates by mutableStateOf<List<VideoSourceFormat>?>(null)
        compose.setContent {
            PicooCameraTheme { SourceFormatSheet(null, candidates, {}, {}) }
        }
        compose.onNodeWithText("正在检查可用格式…").assertIsDisplayed()
        compose.onNodeWithText(VideoSourceFormat.Default.label).assertDoesNotExist()
        compose.runOnIdle { candidates = emptyList() }
        compose.onNodeWithText("当前没有可用格式，请检查镜头、手机方向和接收端。").assertIsDisplayed()
        compose.onNodeWithText(VideoSourceFormat.Default.label).assertDoesNotExist()
    }
}
