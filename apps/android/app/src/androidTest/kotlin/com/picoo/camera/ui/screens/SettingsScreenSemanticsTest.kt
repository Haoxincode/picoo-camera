package com.picoo.camera.ui.screens

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsOff
import androidx.compose.ui.test.assertIsOn
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isToggleable
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import com.picoo.camera.ui.theme.PicooCameraTheme
import org.junit.Rule
import org.junit.Test

class SettingsScreenSemanticsTest {
    @get:Rule
    val composeRule = createComposeRule()

    @Test
    fun autoConnectIsOneToggleableNodeAndReportsItsState() {
        composeRule.setContent {
            var autoConnect by mutableStateOf(true)
            PicooCameraTheme {
                SettingsScreen(
                    pairedDeviceCount = 0,
                    cameraGranted = true,
                    nearbyWifiGranted = true,
                    notificationsGranted = true,
                    autoConnectEnabled = autoConnect,
                    onBack = {},
                    onCheckPermissions = {},
                    onToggleAutoConnect = { autoConnect = !autoConnect },
                )
            }
        }

        composeRule.onAllNodes(isToggleable()).assertCountEquals(1)
        val toggle = composeRule.onNode(hasText("打开 App 自动直连") and isToggleable())
        toggle.assertIsOn()
        toggle.performClick()
        toggle.assertIsOff()
    }

    @Test
    fun settingsDoesNotExposeDefaultQuality() {
        composeRule.setContent {
            PicooCameraTheme {
                SettingsScreen(
                    pairedDeviceCount = 0,
                    cameraGranted = true,
                    nearbyWifiGranted = true,
                    notificationsGranted = true,
                    autoConnectEnabled = true,
                    onBack = {},
                    onCheckPermissions = {},
                    onToggleAutoConnect = {},
                )
            }
        }

        composeRule.onNodeWithText("默认初始画质").assertDoesNotExist()
        composeRule.onNodeWithText("HD", useUnmergedTree = true).assertDoesNotExist()
    }
}
