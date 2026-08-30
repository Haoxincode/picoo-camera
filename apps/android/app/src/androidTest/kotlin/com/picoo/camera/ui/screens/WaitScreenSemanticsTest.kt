package com.picoo.camera.ui.screens

import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import com.picoo.camera.ui.theme.PicooCameraTheme
import org.junit.Rule
import org.junit.Test

class WaitScreenSemanticsTest {
    @get:Rule
    val composeRule = createComposeRule()

    @Test
    fun explicitRejectShowsDangerCopyAndRecoveryAction() {
        composeRule.setContent {
            PicooCameraTheme {
                WaitScreen(
                    receiverName = "Studio PC",
                    outcome = WaitOutcome.Rejected,
                    onCancel = {},
                )
            }
        }

        composeRule.onNodeWithText("电脑端拒绝了连接").assertIsDisplayed()
        composeRule.onNodeWithText("电脑使用者点击了拒绝。确认电脑归属后可重新发起连接。")
            .assertIsDisplayed()
        composeRule.onNodeWithText("返回设备列表").assertIsDisplayed()
    }
}
