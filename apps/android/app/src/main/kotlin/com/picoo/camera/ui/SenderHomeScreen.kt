package com.picoo.camera.ui

import android.view.Surface
import android.view.WindowManager
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import com.picoo.camera.MainActivity
import com.picoo.camera.SenderSessionViewModel
import com.picoo.camera.StreamingBrightness
import com.picoo.camera.StreamingForegroundService
import com.picoo.camera.discovery.NsdReceiverBrowser
import com.picoo.camera.discovery.PairedAutoConnect
import com.picoo.camera.jni.PicooNative
import com.picoo.camera.media.CaptureState
import com.picoo.camera.network.WifiNetworkInfo
import com.picoo.camera.pairing.TrustedDeviceList
import com.picoo.camera.ui.screens.WaitOutcome
import kotlinx.coroutines.delay

@Composable
internal fun SenderHomeScreen(
    sessionModel: SenderSessionViewModel,
    cameraGranted: Boolean,
    cameraPermissionPermanentlyDenied: Boolean,
    nearbyWifiGranted: Boolean,
    notificationsGranted: Boolean,
    onEnsureCamera: () -> Unit,
    onRequestCamera: () -> Unit,
    onRequestNearbyWifi: () -> Unit,
    onRequestNotifications: () -> Unit,
) {
    val context = LocalContext.current
    val senderView = LocalView.current
    val uiState = sessionModel.uiState
    var errorText by uiState::errorText
    var hostText by uiState::hostText
    var portText by uiState::portText
    var senderStatus by uiState::senderStatus
    var pairingCode by uiState::pairingCode
    val discoveredListState = uiState.discoveredReceivers
    val discoveredList = discoveredListState.value
    var pairedDevices by uiState::pairedDevices
    var selectedReceiverId by uiState::selectedReceiverId
    var selectedReceiverName by uiState::selectedReceiverName
    var autoConnectEnabled by uiState::autoConnectEnabled
    var suppressAutoConnect by uiState::suppressAutoConnect
    var encoderState by uiState::encoderState
    var senderTab by uiState::senderTab
    var phonePairingConfirmed by uiState::phonePairingConfirmed
    var discoveryComplete by uiState::discoveryComplete
    var discoveryEnabled by uiState::discoveryEnabled
    var discoverySearchGeneration by uiState::discoverySearchGeneration
    var wifiPillText by uiState::wifiPillText
    var pairingRemainingSeconds by uiState::pairingRemainingSeconds
    var pairingExpired by uiState::pairingExpired
    var pairingStartedAtMs by uiState::pairingStartedAtMs
    var waitOutcome by uiState::waitOutcome
    val runtime = sessionModel.runtime
    val senderHandle = runtime.senderHandle
    val pairedReceiverIds = TrustedDeviceList.ids(pairedDevices)

    fun displayRotationDegrees(): Int = when (senderView.display?.rotation) {
        Surface.ROTATION_90 -> 90
        Surface.ROTATION_180 -> 180
        Surface.ROTATION_270 -> 270
        else -> 0
    }

    LaunchedEffect(senderHandle) {
        (context as? MainActivity)?.bindSenderHandle(senderHandle)
        sessionModel.setDisplayRotationDegrees(displayRotationDegrees())
    }

    LaunchedEffect(cameraGranted) {
        sessionModel.onCameraGrantedChanged(cameraGranted)
    }

    val nsdBrowser =
        remember {
            NsdReceiverBrowser(context) { list ->
                discoveredListState.value = list
                if (list.isNotEmpty()) discoveryComplete = false
            }
        }

    LaunchedEffect(nearbyWifiGranted) {
        if (!nearbyWifiGranted) onRequestNearbyWifi()
    }

    LaunchedEffect(discoveredList, discoverySearchGeneration, discoveryEnabled) {
        if (!discoveryEnabled) {
            discoveryComplete = true
        } else if (discoveredList.isEmpty()) {
            discoveryComplete = false
            delay(3_000)
            if (discoveryEnabled && discoveredListState.value.isEmpty()) {
                discoveryComplete = true
            }
        } else {
            discoveryComplete = false
        }
    }

    LaunchedEffect(Unit) {
        while (true) {
            wifiPillText = WifiNetworkInfo.pillText(context)
            delay(5_000)
        }
    }

    LaunchedEffect(pairingCode, senderTab) {
        if (pairingCode.isNotEmpty() && senderTab == SenderTab.Pairing) {
            pairingStartedAtMs = System.currentTimeMillis()
            pairingExpired = false
            pairingRemainingSeconds = 60
        }
    }

    LaunchedEffect(pairingStartedAtMs, pairingCode, senderTab, pairingExpired) {
        if (pairingCode.isEmpty() || senderTab != SenderTab.Pairing || pairingExpired) {
            return@LaunchedEffect
        }
        while (pairingRemainingSeconds > 0 && senderTab == SenderTab.Pairing) {
            delay(1_000)
            val elapsed = ((System.currentTimeMillis() - pairingStartedAtMs) / 1_000).toInt()
            pairingRemainingSeconds = (60 - elapsed).coerceAtLeast(0)
            if (pairingRemainingSeconds == 0) {
                pairingExpired = true
            }
        }
    }

    LaunchedEffect(phonePairingConfirmed, senderTab) {
        if (!phonePairingConfirmed || senderTab != SenderTab.Wait) {
            return@LaunchedEffect
        }
        waitOutcome = WaitOutcome.Pending
        val waitStartedAt = System.currentTimeMillis()
        while (senderTab == SenderTab.Wait && waitOutcome == WaitOutcome.Pending) {
            delay(1_000)
            val elapsed = ((System.currentTimeMillis() - waitStartedAt) / 1_000).toInt()
            if (elapsed >= 60) {
                waitOutcome = WaitOutcome.Timeout
            }
        }
    }

    DisposableEffect(nsdBrowser, nearbyWifiGranted, discoveryEnabled, discoverySearchGeneration) {
        if (nearbyWifiGranted && discoveryEnabled) nsdBrowser.start()
        onDispose { nsdBrowser.stop() }
    }

    LaunchedEffect(runtime) {
        if (runtime.attachTrustedStore() != 0) {
            errorText = "无法加载配对存储"
        }
        sessionModel.reloadTrustedStore()
    }

    LaunchedEffect(
        discoveredList,
        pairedReceiverIds,
        senderStatus,
        autoConnectEnabled,
        suppressAutoConnect,
    ) {
        if (!autoConnectEnabled || suppressAutoConnect) return@LaunchedEffect
        val sessionBusy = when (senderStatus) {
            PicooNative.STATUS_DISCONNECTED,
            PicooNative.STATUS_DISCOVERING,
            -> false
            else -> true
        }
        val pick = PairedAutoConnect.pick(
            discovered = discoveredList,
            pairedReceiverIds = pairedReceiverIds,
            sessionBusy = sessionBusy,
            alreadyAttemptedIds = runtime.autoConnectAttempts(),
        ) ?: return@LaunchedEffect
        runtime.markAutoConnectAttempt(pick.receiverId)
        hostText = pick.host
        portText = pick.quicPort.toString()
        selectedReceiverId = pick.receiverId
        selectedReceiverName = pick.displayName
        sessionModel.connect(pick.host, pick.quicPort, pick.receiverId)
    }

    val keepScreenOn = when (senderStatus) {
        PicooNative.STATUS_STREAMING,
        PicooNative.STATUS_NEGOTIATING,
        PicooNative.STATUS_RECONNECTING,
        PicooNative.STATUS_NETWORK_UNSTABLE,
        -> true
        else -> false
    }
    val dimForStreaming = senderTab == SenderTab.Streaming && keepScreenOn
    val view = LocalView.current
    androidx.compose.runtime.SideEffect {
        sessionModel.setDisplayRotationDegrees(displayRotationDegrees())
        val window = (view.context as? android.app.Activity)?.window
        if (keepScreenOn) {
            window?.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        } else {
            window?.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        }
        window?.let { w ->
            val attrs = w.attributes
            val target = if (dimForStreaming) {
                StreamingBrightness.DIMMED
            } else {
                WindowManager.LayoutParams.BRIGHTNESS_OVERRIDE_NONE
            }
            if (attrs.screenBrightness != target) {
                attrs.screenBrightness = target
                w.attributes = attrs
            }
        }
    }

    LaunchedEffect(senderStatus, cameraGranted, encoderState) {
        val encoding = encoderState == CaptureState.Previewing
        val liveEncode = cameraGranted && encoding && when (senderStatus) {
            PicooNative.STATUS_STREAMING,
            PicooNative.STATUS_RECONNECTING,
            PicooNative.STATUS_NETWORK_UNSTABLE,
            -> true
            else -> false
        }
        if (liveEncode) {
            onRequestNotifications()
            StreamingForegroundService.start(context)
        } else {
            StreamingForegroundService.stop(context)
        }
    }

    LaunchedEffect(senderStatus, cameraGranted) {
        val liveSession = sessionModel.isLiveSession(senderStatus)
        if (liveSession && !cameraGranted) {
            onEnsureCamera()
        }
    }

    LaunchedEffect(senderStatus, pairingCode, phonePairingConfirmed, senderTab) {
        when {
            senderTab == SenderTab.Settings -> Unit
            senderStatus == PicooNative.STATUS_STREAMING ||
                senderStatus == PicooNative.STATUS_NETWORK_UNSTABLE ||
                senderStatus == PicooNative.STATUS_PERMISSION_REQUIRED -> {
                phonePairingConfirmed = false
                senderTab = SenderTab.Streaming
            }
            phonePairingConfirmed &&
                senderTab != SenderTab.Streaming &&
                senderStatus != PicooNative.STATUS_DISCONNECTED ->
                senderTab = SenderTab.Wait
            (pairingCode.isNotEmpty() || senderStatus == PicooNative.STATUS_PAIRING) &&
                !phonePairingConfirmed ->
                senderTab = SenderTab.Pairing
            waitOutcome != WaitOutcome.Pending &&
                senderTab == SenderTab.Wait -> Unit
        }
    }

    Box(modifier = Modifier.fillMaxSize()) {
        SenderTabContent(
            sessionModel = sessionModel,
            cameraGranted = cameraGranted,
            cameraPermissionPermanentlyDenied = cameraPermissionPermanentlyDenied,
            nearbyWifiGranted = nearbyWifiGranted,
            notificationsGranted = notificationsGranted,
            onRequestCamera = onRequestCamera,
            onRequestNearbyWifi = onRequestNearbyWifi,
            onRequestNotifications = onRequestNotifications,
            displayRotationDegrees = ::displayRotationDegrees,
        )
    }
}
