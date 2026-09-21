package com.picoo.camera.ui

import androidx.compose.runtime.Composable
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import com.picoo.camera.SenderSessionViewModel
import com.picoo.camera.jni.PicooNative
import com.picoo.camera.media.ExposureCompensation
import com.picoo.camera.media.LensFacing
import com.picoo.camera.media.LocalPreviewMirror
import com.picoo.camera.pairing.TrustedDeviceList
import com.picoo.camera.ui.screens.ConnectionSheet
import com.picoo.camera.ui.screens.PairingCameraOverlay
import com.picoo.camera.ui.screens.PairingWaitCameraOverlay
import com.picoo.camera.ui.screens.SettingsScreen
import com.picoo.camera.ui.screens.SourceFormatSheet
import com.picoo.camera.ui.screens.StreamingScreen
import com.picoo.camera.ui.screens.WaitOutcome

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
    var showSourceSheet by rememberSaveable { mutableStateOf(false) }
    var connectionSheetOpen by rememberSaveable { mutableStateOf(false) }
    var powerHint by uiState::powerHint
    var thermalLimited by uiState::thermalLimited
    var linkQualityChip by uiState::linkQualityChip
    var adaptiveBitrateBps by uiState::adaptiveBitrateBps
    var exposureEv by uiState::exposureEv
    var senderTab by uiState::senderTab
    var phonePairingConfirmed by uiState::phonePairingConfirmed
    var discoveryComplete by uiState::discoveryComplete
    var discoveryEnabled by uiState::discoveryEnabled
    var discoverySearchGeneration by uiState::discoverySearchGeneration
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

    val connected = senderStatus in setOf(
        PicooNative.STATUS_STREAMING,
        PicooNative.STATUS_RECONNECTING,
        PicooNative.STATUS_NETWORK_UNSTABLE,
        PicooNative.STATUS_PERMISSION_REQUIRED,
    )
    val connectionTitle = when {
        senderStatus == PicooNative.STATUS_PERMISSION_REQUIRED -> "需要相机权限"
        senderStatus == PicooNative.STATUS_RECONNECTING -> "正在重连电脑"
        connected -> "${pairingDisplayName.ifBlank { "Picoo Camera" }} 已连接"
        senderStatus == PicooNative.STATUS_PAIRING -> "等待配对确认"
        senderStatus == PicooNative.STATUS_CONNECTING ||
            senderStatus == PicooNative.STATUS_NEGOTIATING -> "正在连接电脑"
        discoveryComplete -> "点击连接电脑"
        else -> "正在寻找电脑…"
    }
    val connectionDetail = when {
        senderStatus == PicooNative.STATUS_PERMISSION_REQUIRED -> "允许相机权限后开始预览"
        senderStatus == PicooNative.STATUS_RECONNECTING -> "连接恢复后会自动继续推流"
        connected -> "已建立安全连接"
        errorText != null -> errorText.orEmpty()
        discoveryComplete -> "自动发现或输入局域网 IP"
        else -> "保持手机与电脑连接同一 Wi‑Fi"
    }

    if (senderTab == SenderTab.Settings) {
        SettingsScreen(
            pairedDeviceCount = pairedDevices.size,
            pairedDevices = pairedDevices,
            errorText = errorText,
            cameraGranted = cameraGranted,
            nearbyWifiGranted = nearbyWifiGranted,
            notificationsGranted = notificationsGranted,
            autoConnectEnabled = autoConnectEnabled,
            onBack = { senderTab = SenderTab.Streaming },
            onCheckPermissions = {
                onRequestNearbyWifi()
                onRequestNotifications()
                onRequestCamera()
            },
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
        )
    } else {
        StreamingScreen(
            // Camera2 owns the selected buffer geometry; Compose only transforms it.
            cameraGranted = cameraGranted,
            cameraPermissionPermanentlyDenied = cameraPermissionPermanentlyDenied,
            receiverName = pairingDisplayName,
            linkQualityChip = linkQualityChip,
            sourceLabel = uiState.committedSourceFormat?.label ?: "等待源格式提交",
            errorText = errorText,
            bitrateMbps = bitrateMbps,
            previewBufferWidth = previewTransformInfo.bufferSize.width,
            previewBufferHeight = previewTransformInfo.bufferSize.height,
            previewSensorOrientationDegrees =
                previewTransformInfo.sensorOrientationDegrees,
            previewFrontFacing =
                previewTransformInfo.lensFacing == LensFacing.Front,
            localPreviewMirrored = localPreviewMirrored,
            thermalLimited = thermalLimited,
            powerHint = if (thermalLimited) "" else powerHint,
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
            onFlipCamera = { sessionModel.requestCameraSwitch() },
            onChooseSourceFormat = { showSourceSheet = true },
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
            connected = connected,
            connectionTitle = connectionTitle,
            connectionDetail = connectionDetail,
            onConnectionClick = { connectionSheetOpen = true },
            onOpenSettings = {
                connectionSheetOpen = false
                senderTab = SenderTab.Settings
            },
            onConnect = { connectionSheetOpen = true },
            connectionOverlay = when (senderTab) {
                SenderTab.Pairing -> {
                    {
                        PairingCameraOverlay(
                            receiverName = pairingDisplayName,
                            pairingCode = pairingCode,
                            remainingSeconds = pairingRemainingSeconds,
                            expired = pairingExpired,
                            errorText = errorText,
                            onConfirm = {
                                if (!pairingExpired) {
                                    val receiverId = connectedReceiverId.ifEmpty {
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
                    }
                }
                SenderTab.Wait -> {
                    {
                        PairingWaitCameraOverlay(
                            receiverName = pairingDisplayName,
                            outcome = waitOutcome,
                            onCancel = {
                                PicooNative.disconnect(senderHandle)
                                phonePairingConfirmed = false
                                suppressAutoConnect = true
                                sessionModel.resetToDevices()
                            },
                            onRetry = { sessionModel.regeneratePairing() },
                        )
                    }
                }
                else -> null
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
    if (connectionSheetOpen && senderTab != SenderTab.Settings) {
        ConnectionSheet(
            discoveredList = discoveredList,
            pairedDevices = pairedDevices,
            pairedReceiverIds = pairedReceiverIds,
            nearbyWifiGranted = nearbyWifiGranted,
            discoveryComplete = discoveryComplete,
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
                connectionSheetOpen = false
            },
            onManualConnect = { host, port ->
                sessionModel.rememberManualEndpoint(host, port)
                hostText = host
                portText = port.toString()
                selectedReceiverId = "manual-$host"
                selectedReceiverName = host
                sessionModel.connect(host, port, selectedReceiverId)
                connectionSheetOpen = false
            },
            onManualEndpointChange = {
                manualEndpointText = it
                errorText = null
            },
            onCheckPermissions = {
                onRequestNearbyWifi()
                onRequestNotifications()
            },
            onRequestNearbyWifi = onRequestNearbyWifi,
            onRestartDiscovery = {
                discoveryEnabled = true
                discoveryComplete = false
                discoverySearchGeneration += 1
            },
            onStopDiscovery = {
                discoveryEnabled = false
                discoveryComplete = true
            },
            onDismiss = { connectionSheetOpen = false },
        )
    }
    if (showSourceSheet && connected) {
        SourceFormatSheet(
            selected = uiState.committedSourceFormat,
            candidates = uiState.availableSourceFormats,
            preparationError = uiState.sourcePreparationError,
            onDismiss = { showSourceSheet = false },
            onSelect = { source ->
                if (sessionModel.requestSourceFormat(source)) showSourceSheet = false
            },
        )
    }

}
