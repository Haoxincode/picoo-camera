package com.picoo.camera.ui

import android.widget.Toast
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext
import com.picoo.camera.SenderSessionViewModel
import com.picoo.camera.jni.PicooNative
import com.picoo.camera.media.ExposureCompensation
import com.picoo.camera.media.LensFacing
import com.picoo.camera.media.LocalPreviewMirror
import com.picoo.camera.media.StreamResolution
import com.picoo.camera.pairing.TrustedDeviceList
import com.picoo.camera.ui.screens.DevicesScreen
import com.picoo.camera.ui.screens.PairingScreen
import com.picoo.camera.ui.screens.SettingsScreen
import com.picoo.camera.ui.screens.StreamingScreen
import com.picoo.camera.ui.screens.WaitOutcome
import com.picoo.camera.ui.screens.WaitScreen

@Composable
internal fun SenderTabContent(
    sessionModel: SenderSessionViewModel,
    cameraGranted: Boolean,
    cameraPermissionPermanentlyDenied: Boolean,
    nearbyWifiGranted: Boolean,
    notificationsGranted: Boolean,
    onRequestCamera: () -> Unit,
    onRequestNearbyWifi: () -> Unit,
    onRequestNotifications: () -> Unit,
    displayRotationDegrees: () -> Int,
) {
    val context = LocalContext.current
    val uiState = sessionModel.uiState
    var errorText by uiState::errorText
    var hostText by uiState::hostText
    var portText by uiState::portText
    var manualEndpointText by uiState::manualEndpointText
    var senderStatus by uiState::senderStatus
    var pairingCode by uiState::pairingCode
    val discoveredList = uiState.discoveredReceivers.value
    var connectedReceiverId by uiState::connectedReceiverId
    var connectedReceiverName by uiState::connectedReceiverName
    var pairedDevices by uiState::pairedDevices
    var selectedReceiverId by uiState::selectedReceiverId
    var selectedReceiverName by uiState::selectedReceiverName
    var autoConnectEnabled by uiState::autoConnectEnabled
    var suppressAutoConnect by uiState::suppressAutoConnect
    var localPreviewMirrored by uiState::localPreviewMirrored
    var resolutionLabel by uiState::resolutionLabel
    var preferredResolutionLabel by uiState::preferredResolutionLabel
    var powerHint by uiState::powerHint
    var thermalForced720 by uiState::thermalForced720
    var linkQualityChip by uiState::linkQualityChip
    var adaptiveBitrateBps by uiState::adaptiveBitrateBps
    var exposureEv by uiState::exposureEv
    var senderTab by uiState::senderTab
    var phonePairingConfirmed by uiState::phonePairingConfirmed
    var discoveryComplete by uiState::discoveryComplete
    var discoveryEnabled by uiState::discoveryEnabled
    var discoverySearchGeneration by uiState::discoverySearchGeneration
    var wifiPillText by uiState::wifiPillText
    var pairingRemainingSeconds by uiState::pairingRemainingSeconds
    var pairingExpired by uiState::pairingExpired
    var waitOutcome by uiState::waitOutcome
    var reconnectAttempt by uiState::reconnectAttempt
    var reconnectDelayMs by uiState::reconnectDelayMs
    var previewTransformInfo by uiState::previewTransformInfo
    var encoderState by uiState::encoderState
    val runtime = sessionModel.runtime
    val encoder = sessionModel.encoder
    val senderHandle = runtime.senderHandle
    val pairedReceiverIds = TrustedDeviceList.ids(pairedDevices)
    val bitrateMbps = "%.1f Mbps".format(adaptiveBitrateBps / 1_000_000.0)
    val pairingDisplayName = selectedReceiverName.ifBlank { connectedReceiverName }

    when (senderTab) {
        SenderTab.Devices -> DevicesScreen(
            discoveredList = discoveredList,
            pairedDevices = pairedDevices,
            pairedReceiverIds = pairedReceiverIds,
            nearbyWifiGranted = nearbyWifiGranted,
            discoveryComplete = discoveryComplete,
            wifiPillText = wifiPillText,
            errorText = errorText,
            manualEndpointText = manualEndpointText,
            manualConnecting = uiState.connectionStartedAtMs != 0L &&
                selectedReceiverId.startsWith("manual-") &&
                senderStatus in setOf(
                    PicooNative.STATUS_CONNECTING,
                    PicooNative.STATUS_NEGOTIATING,
                    PicooNative.STATUS_RECONNECTING,
                ),
            onSelectReceiver = { receiver ->
                selectedReceiverName = receiver.displayName
                hostText = receiver.host
                portText = receiver.quicPort.toString()
                selectedReceiverId = receiver.receiverId
                sessionModel.connect(receiver.host, receiver.quicPort, receiver.receiverId)
            },
            onManualConnect = { host, port ->
                sessionModel.rememberManualEndpoint(host, port)
                hostText = host
                portText = port.toString()
                selectedReceiverId = "manual-$host"
                selectedReceiverName = host
                sessionModel.connect(host, port, selectedReceiverId)
            },
            onManualEndpointChange = {
                manualEndpointText = it
                errorText = null
            },
            onCheckPermissions = {
                onRequestNearbyWifi()
                onRequestNotifications()
            },
            onRemovePaired = { device ->
                selectedReceiverId = device.deviceId
                val rc = runtime.removeTrustedDevice(device.deviceId)
                if (rc == 1) {
                    runtime.forgetAutoConnectAttempt(device.deviceId)
                    if (sessionModel.reloadTrustedStore()) errorText = null
                } else {
                    errorText = "删除失败 ($rc)"
                }
            },
            onOfflinePairedClick = { device ->
                Toast.makeText(
                    context,
                    "${device.deviceName} 当前不在线，请确认电脑端 Picoo Camera 已启动",
                    Toast.LENGTH_SHORT,
                ).show()
            },
            onRequestNearbyWifi = onRequestNearbyWifi,
            onOpenSettings = { senderTab = SenderTab.Settings },
            onRestartDiscovery = {
                discoveryEnabled = true
                discoveryComplete = false
                discoverySearchGeneration += 1
            },
            onStopDiscovery = {
                discoveryEnabled = false
                discoveryComplete = true
            },
        )
        SenderTab.Settings -> SettingsScreen(
            pairedDeviceCount = pairedDevices.size,
            pairedDevices = pairedDevices,
            errorText = errorText,
            cameraGranted = cameraGranted,
            nearbyWifiGranted = nearbyWifiGranted,
            notificationsGranted = notificationsGranted,
            autoConnectEnabled = autoConnectEnabled,
            defaultResolutionLabel = preferredResolutionLabel,
            onBack = { senderTab = SenderTab.Devices },
            onCheckPermissions = {
                onRequestNearbyWifi()
                onRequestNotifications()
                onRequestCamera()
            },
            onOpenPairedDevices = { senderTab = SenderTab.Devices },
            onRemovePaired = { device ->
                val rc = runtime.removeTrustedDevice(device.deviceId)
                if (rc == 1) {
                    runtime.forgetAutoConnectAttempt(device.deviceId)
                    if (sessionModel.reloadTrustedStore()) errorText = null
                } else {
                    errorText = "删除失败 ($rc)"
                }
            },
            onToggleAutoConnect = {
                sessionModel.setAutoConnectEnabled(!autoConnectEnabled)
            },
            onSelectDefaultResolution = { label ->
                sessionModel.setPreferredResolution(StreamResolution.fromLabel(label))
            },
        )
        SenderTab.Pairing -> PairingScreen(
            receiverName = pairingDisplayName,
            pairingCode = pairingCode,
            remainingSeconds = pairingRemainingSeconds,
            expired = pairingExpired,
            errorText = errorText,
            onConfirm = {
                if (pairingExpired) return@PairingScreen
                val receiverId =
                    connectedReceiverId.ifEmpty {
                        selectedReceiverId.ifEmpty { "windows-receiver" }
                    }
                val rc = PicooNative.sendPairingConfirm(senderHandle, receiverId)
                if (rc == 0) {
                    errorText = null
                    phonePairingConfirmed = true
                    waitOutcome = WaitOutcome.Pending
                    senderTab = SenderTab.Wait
                } else {
                    errorText = "配对确认失败 ($rc)"
                }
            },
            onRegenerate = { sessionModel.regeneratePairing() },
            onCancel = {
                PicooNative.disconnect(senderHandle)
                pairingCode = ""
                connectedReceiverId = ""
                connectedReceiverName = ""
                phonePairingConfirmed = false
                pairingExpired = false
                suppressAutoConnect = true
                sessionModel.resetToDevices()
            },
        )
        SenderTab.Wait -> WaitScreen(
            receiverName = pairingDisplayName,
            outcome = waitOutcome,
            onCancel = {
                PicooNative.disconnect(senderHandle)
                phonePairingConfirmed = false
                suppressAutoConnect = true
                sessionModel.resetToDevices()
            },
            onBackToDevices = {
                PicooNative.disconnect(senderHandle)
                phonePairingConfirmed = false
                suppressAutoConnect = true
                sessionModel.resetToDevices()
            },
            onRegenerate = { sessionModel.regeneratePairing() },
        )
        SenderTab.Streaming -> StreamingScreen(
            // Camera2 owns the selected buffer geometry; Compose only transforms it.
            cameraGranted = cameraGranted,
            cameraPermissionPermanentlyDenied = cameraPermissionPermanentlyDenied,
            receiverName = pairingDisplayName,
            linkQualityChip = linkQualityChip,
            resolutionLabel = resolutionLabel,
            bitrateMbps = bitrateMbps,
            previewBufferWidth = previewTransformInfo.bufferSize.width,
            previewBufferHeight = previewTransformInfo.bufferSize.height,
            previewSensorOrientationDegrees =
                previewTransformInfo.sensorOrientationDegrees,
            previewFrontFacing =
                previewTransformInfo.lensFacing == LensFacing.Front,
            localPreviewMirrored = localPreviewMirrored,
            thermalForced720 = thermalForced720,
            powerHint = if (thermalForced720) "" else powerHint,
            reconnecting = senderStatus == PicooNative.STATUS_RECONNECTING ||
                senderStatus == PicooNative.STATUS_NETWORK_UNSTABLE,
            networkUnstable = senderStatus == PicooNative.STATUS_NETWORK_UNSTABLE,
            reconnectAttempt = reconnectAttempt,
            reconnectDelayMs = reconnectDelayMs,
            packetLossLabel = run {
                val link = PicooNative.getLinkStats(senderHandle)
                if (link != null && link.size >= 2) {
                    val lossPct = (link[1] * 100).toInt()
                    "$lossPct% 丢包"
                } else {
                    "0% 丢包"
                }
            },
            onRequestCamera = onRequestCamera,
            onFlipCamera = {
                if (sessionModel.beginLocalEncoderReconfiguration(encoder.profile.resolution.height)) {
                    encoder.switchCamera()
                    previewTransformInfo = encoder.previewTransformInfo
                    localPreviewMirrored =
                        LocalPreviewMirror.defaultFor(encoder.profile.lensFacing)
                    encoderState = encoder.state
                    sessionModel.streamConfigDirty.set(true)
                }
            },
            onToggleResolution = {
                val current = StreamResolution.fromLabel(resolutionLabel)
                val next = StreamResolution.next(current, thermalForced720)
                val maxH = PicooNative.readSenderSnapshot(senderHandle).receiverMaxHeight
                if (maxH in 1 until next.height) {
                    errorText = "接收端最高 ${maxH}p — 无法切换至 ${next.label}"
                    return@StreamingScreen
                }
                if (thermalForced720 && next == StreamResolution.P1080) {
                    errorText = "设备过热，暂不可升 1080p"
                    Toast.makeText(
                        context,
                        "设备偏热保护中，1080P 暂不可选",
                        Toast.LENGTH_SHORT,
                    ).show()
                    return@StreamingScreen
                }
                val bitrate = PicooNative.bitrateInitialForHeight(next.height)
                if (sessionModel.beginLocalEncoderReconfiguration(next.height)) {
                    resolutionLabel = next.label
                    PicooNative.setPreferredHeight(senderHandle, next.height)
                    encoder.setTargetBitrateBps(bitrate)
                    encoder.setResolution(next.width, next.height)
                    previewTransformInfo = encoder.previewTransformInfo
                    encoderState = encoder.state
                    sessionModel.streamConfigDirty.set(true)
                    errorText = null
                }
            },
            onToggleMirror = { localPreviewMirrored = !localPreviewMirrored },
            onCycleExposure = {
                val range = encoder.exposureCompensationRange
                if (range.isEmpty()) return@StreamingScreen
                encoder.setExposureCompensation(
                    ExposureCompensation.nextControlPreset(exposureEv, range),
                )
                exposureEv = encoder.exposureCompensation
            },
            exposureEv = exposureEv,
            evSupported = !encoder.exposureCompensationRange.isEmpty(),
            onDisconnect = {
                suppressAutoConnect = true
                PicooNative.disconnect(senderHandle)
                senderStatus = PicooNative.readSenderSnapshot(senderHandle).status
                sessionModel.resetToDevices()
            },
            onStopReconnect = {
                suppressAutoConnect = true
                PicooNative.disconnect(senderHandle)
                senderStatus = PicooNative.readSenderSnapshot(senderHandle).status
                sessionModel.resetToDevices()
            },
            onPreviewSurfaceAvailable = { surface ->
                sessionModel.setDisplayRotationDegrees(displayRotationDegrees())
                encoder.bindPreviewSurface(surface)
                previewTransformInfo = encoder.previewTransformInfo
                encoderState = encoder.state
                errorText = encoder.lastError
            },
            onPreviewSurfaceDestroyed = { surfaceTexture ->
                encoder.unbindPreviewSurface(surfaceTexture)
            },
            onPreviewDisplayChanged = {
                sessionModel.setDisplayRotationDegrees(displayRotationDegrees())
                previewTransformInfo = encoder.refreshPreviewTransformInfo()
            },
        )
    }
}
