package com.picoo.camera.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.assertTextEquals
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import com.picoo.camera.ui.theme.PicooCameraColors
import com.picoo.camera.ui.theme.PicooCameraTheme
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test

class StreamingScreenSemanticsTest {
    @get:Rule
    val composeRule = createComposeRule()

    @Test
    fun connectedControlsUseEqualWidthsWithinEachRow() {
        setConnectedContent()

        val exposureWidth = controlWidth("调节亮度")
        val mirrorWidth = controlWidth("切换本机预览镜像")
        val lockWidth = controlWidth("防误触锁定")
        val disconnectWidth = controlWidth("断开连接")
        val switchCameraWidth = controlWidth("切换前后摄像头")

        assertEquals(exposureWidth, mirrorWidth, 1f)
        assertEquals(exposureWidth, lockWidth, 1f)
        assertEquals(disconnectWidth, switchCameraWidth, 1f)
    }

    @Test
    fun lockDisablesCameraActionsButRemainsAvailableToUnlock() {
        setConnectedContent()

        composeRule.onNodeWithContentDescription("防误触锁定").performClick()

        composeRule.onNodeWithContentDescription("调节亮度").assertIsNotEnabled()
        composeRule.onNodeWithContentDescription("切换本机预览镜像").assertIsNotEnabled()
        composeRule.onNodeWithContentDescription("断开连接").assertIsNotEnabled()
        composeRule.onNodeWithContentDescription("切换前后摄像头").assertIsNotEnabled()
        composeRule.onNodeWithContentDescription("防误触锁定").assertIsEnabled().performClick()
        composeRule.onNodeWithContentDescription("调节亮度").assertIsEnabled()
    }

    @Test
    fun disconnectRequiresTwoClicks() {
        var disconnectCount = 0
        setConnectedContent(onDisconnect = { disconnectCount += 1 })

        composeRule.onNodeWithContentDescription("断开连接").performClick()
        composeRule.onNodeWithText("再次点击确认断开").assertTextEquals("再次点击确认断开")
        composeRule.runOnIdle { assertEquals(0, disconnectCount) }

        composeRule.onNodeWithContentDescription("断开连接").performClick()
        composeRule.runOnIdle { assertEquals(1, disconnectCount) }
    }

    @Test
    fun permanentlyDeniedCameraPermissionExplainsSettingsAction() {
        var requestCount = 0
        setConnectedContent(
            cameraGranted = false,
            cameraPermissionPermanentlyDenied = true,
            onRequestCamera = { requestCount += 1 },
        )

        composeRule.onNodeWithText("相机权限已关闭，请前往系统设置开启后继续推流")
            .assertTextEquals("相机权限已关闭，请前往系统设置开启后继续推流")
        composeRule.onNodeWithText("前往设置").performClick()
        composeRule.runOnIdle { assertEquals(1, requestCount) }
    }

    @Test
    fun connectedPreviewIsFullScreenWithoutAReferenceFrame() {
        setConnectedContent()

        composeRule.onNodeWithContentDescription("本机相机预览").fetchSemanticsNode()
        assertEquals(
            0,
            composeRule.onAllNodesWithContentDescription("16:9 电脑端输出范围")
                .fetchSemanticsNodes().size,
        )
        assertEquals(
            0,
            composeRule.onAllNodesWithText("横屏可获得更大的预览画面")
                .fetchSemanticsNodes().size,
        )
    }

    private fun controlWidth(contentDescription: String): Float =
        composeRule.onNodeWithContentDescription(contentDescription)
            .fetchSemanticsNode()
            .boundsInRoot
            .width

    private fun setConnectedContent(
        cameraGranted: Boolean = true,
        cameraPermissionPermanentlyDenied: Boolean = false,
        onRequestCamera: () -> Unit = {},
        onDisconnect: () -> Unit = {},
    ) {
        composeRule.setContent {
            PicooCameraTheme {
                StreamingScreenContent(
                    cameraGranted = cameraGranted,
                    cameraPermissionPermanentlyDenied = cameraPermissionPermanentlyDenied,
                    receiverName = "Studio PC",
                    linkQualityChip = "稳定 · 63ms",
                    resolutionLabel = "720p",
                    bitrateMbps = "1.9 Mbps",
                    localPreviewMirrored = false,
                    thermalForced720 = false,
                    powerHint = "",
                    reconnecting = false,
                    packetLossLabel = "0% 丢包",
                    onRequestCamera = onRequestCamera,
                    onFlipCamera = {},
                    onToggleResolution = {},
                    onToggleMirror = {},
                    onCycleExposure = {},
                    exposureEv = 0,
                    evSupported = true,
                    onDisconnect = onDisconnect,
                    onStopReconnect = {},
                    previewContent = {
                        Box(
                            modifier = Modifier
                                .fillMaxSize()
                                .background(PicooCameraColors.SurfaceRaised),
                        )
                    },
                )
            }
        }
    }
}
