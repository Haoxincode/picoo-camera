package com.picoo.camera.ui.screens

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import com.picoo.camera.ui.theme.PicooCameraTheme
import org.junit.Rule
import org.junit.Test

class DevicesScreenSemanticsTest {
    @get:Rule
    val composeRule = createComposeRule()

    @Test
    fun emptyDiscoveryExposesStopAndRestartActions() {
        composeRule.setContent {
            var discoveryComplete by mutableStateOf(false)
            PicooCameraTheme {
                DevicesScreen(
                    discoveredList = emptyList(),
                    pairedDevices = emptyList(),
                    pairedReceiverIds = emptySet(),
                    nearbyWifiGranted = true,
                    discoveryComplete = discoveryComplete,
                    wifiPillText = "Wi‑Fi",
                    errorText = null,
                    manualEndpointText = "",
                    manualConnecting = false,
                    onSelectReceiver = {},
                    onManualConnect = { _, _ -> },
                    onManualEndpointChange = {},
                    onCheckPermissions = {},
                    onRemovePaired = {},
                    onOfflinePairedClick = {},
                    onRequestNearbyWifi = {},
                    onOpenSettings = {},
                    onRestartDiscovery = { discoveryComplete = false },
                    onStopDiscovery = { discoveryComplete = true },
                )
            }
        }

        composeRule.onNodeWithText("停止搜索").performClick()
        composeRule.onNodeWithText("未发现附近电脑").assertIsDisplayed()
        composeRule.onNodeWithText("重新搜索附近电脑").performClick()
        composeRule.onNodeWithText("正在查找附近电脑").assertIsDisplayed()
    }
}
