package com.picoo.camera.ui.screens

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.assertTextEquals
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import com.picoo.camera.ui.theme.PicooCameraTheme
import org.junit.Rule
import org.junit.Assert.assertEquals
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

    @Test
    fun pastedManualEndpointIsDistributedIntoAddressSegments() {
        composeRule.setContent {
            var endpoint by mutableStateOf("")
            PicooCameraTheme {
                DevicesScreen(
                    discoveredList = emptyList(),
                    pairedDevices = emptyList(),
                    pairedReceiverIds = emptySet(),
                    nearbyWifiGranted = true,
                    discoveryComplete = true,
                    wifiPillText = "Wi‑Fi",
                    errorText = null,
                    manualEndpointText = endpoint,
                    manualConnecting = false,
                    onSelectReceiver = {},
                    onManualConnect = { _, _ -> },
                    onManualEndpointChange = { endpoint = it },
                    onCheckPermissions = {},
                    onRemovePaired = {},
                    onOfflinePairedClick = {},
                    onRequestNearbyWifi = {},
                    onOpenSettings = {},
                    onRestartDiscovery = {},
                    onStopDiscovery = {},
                )
            }
        }

        composeRule.onNodeWithText("通过 IP 地址连接").performClick()
        composeRule.onNodeWithContentDescription("IP 地址第 1 段")
            .performClick()
            .performTextInput("192.168.1.108:8443")

        composeRule.onNodeWithContentDescription("IP 地址第 1 段").assertTextEquals("192")
        composeRule.onNodeWithContentDescription("IP 地址第 2 段").assertTextEquals("168")
        composeRule.onNodeWithContentDescription("IP 地址第 3 段").assertTextEquals("1")
        composeRule.onNodeWithContentDescription("IP 地址第 4 段").assertTextEquals("108")
        composeRule.onNodeWithContentDescription("端口").assertTextEquals("8443").assertIsFocused()
    }

    @Test
    fun missingWifiPermissionUsesTheDiscoveryPanelWithoutAddingAnotherBlock() {
        var permissionRequests = 0
        composeRule.setContent {
            PicooCameraTheme {
                DevicesScreen(
                    discoveredList = emptyList(),
                    pairedDevices = emptyList(),
                    pairedReceiverIds = emptySet(),
                    nearbyWifiGranted = false,
                    discoveryComplete = false,
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
                    onRequestNearbyWifi = { permissionRequests += 1 },
                    onOpenSettings = {},
                    onRestartDiscovery = {},
                    onStopDiscovery = {},
                )
            }
        }

        composeRule.onNodeWithText("允许查找附近电脑").assertIsDisplayed()
        composeRule.onNodeWithText("正在查找附近电脑").assertDoesNotExist()
        composeRule.onNodeWithText("允许查找电脑").performClick()
        composeRule.runOnIdle { assertEquals(1, permissionRequests) }
    }
}
