package com.picoo.camera.ui.screens

import androidx.compose.runtime.Composable
import androidx.compose.ui.tooling.preview.Devices
import androidx.compose.ui.tooling.preview.Preview
import com.picoo.camera.ui.theme.PicooCameraTheme

@Preview(name = "Phone", device = Devices.PHONE, showBackground = true)
@Preview(name = "Foldable", device = Devices.FOLDABLE, showBackground = true)
@Preview(name = "Tablet", device = Devices.TABLET, showBackground = true)
private annotation class PicooFormFactorPreviews

@PicooFormFactorPreviews
@Composable
private fun PairingScreenPreview() {
    PicooCameraTheme {
        PairingScreen(
            receiverName = "Studio PC",
            pairingCode = "482917",
            remainingSeconds = 42,
            expired = false,
            errorText = null,
            onConfirm = {},
            onRegenerate = {},
            onCancel = {},
        )
    }
}

@PicooFormFactorPreviews
@Composable
private fun SettingsScreenPreview() {
    PicooCameraTheme {
        SettingsScreen(
            pairedDeviceCount = 2,
            cameraGranted = true,
            nearbyWifiGranted = true,
            notificationsGranted = false,
            autoConnectEnabled = true,
            defaultResolutionLabel = "1080P",
            onBack = {},
            onCheckPermissions = {},
            onOpenPairedDevices = {},
            onToggleAutoConnect = {},
            onSelectDefaultResolution = {},
        )
    }
}
