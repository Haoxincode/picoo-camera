package com.picoo.camera.ui

import androidx.compose.runtime.Stable
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import com.picoo.camera.jni.PicooNative
import com.picoo.camera.media.CaptureState
import com.picoo.camera.media.LensFacing
import com.picoo.camera.media.LocalPreviewMirror
import com.picoo.camera.ui.screens.WaitOutcome

/**
 * Screen-level UI state holder for the Sender flow.
 *
 * Protocol/session truth remains in Rust and encoder lifecycle truth remains in
 * [com.picoo.camera.media.Camera2MediaEncoder]; this object only owns values
 * rendered or edited by Compose (REQ-PICOO-UI-010).
 */
@Stable
class SenderHomeState {
    var encoderState by mutableStateOf(CaptureState.Idle)
    var errorText by mutableStateOf<String?>(null)
    var hostText by mutableStateOf("127.0.0.1")
    var portText by mutableStateOf("4433")
    var senderStatus by mutableIntStateOf(PicooNative.STATUS_DISCONNECTED)
    var pairingCode by mutableStateOf("")
    val discoveredReceivers = mutableStateOf<List<PicooNative.DiscoveredReceiver>>(emptyList())
    var connectedReceiverId by mutableStateOf("")
    var connectedReceiverName by mutableStateOf("")
    var pairedDevices by mutableStateOf<List<PicooNative.TrustedDevice>>(emptyList())
    var selectedReceiverId by mutableStateOf("")
    var selectedReceiverName by mutableStateOf("")
    var autoConnectEnabled by mutableStateOf(true)
    var suppressAutoConnect by mutableStateOf(false)
    var remoteMirrored by mutableStateOf(false)
    var localPreviewMirrored by mutableStateOf(LocalPreviewMirror.defaultFor(LensFacing.Back))
    var resolutionLabel by mutableStateOf("720p")
    var preferredResolutionLabel by mutableStateOf("1080p")
    var powerHint by mutableStateOf("")
    var thermalForced720 by mutableStateOf(false)
    var thermalToastShown by mutableStateOf(false)
    var linkQualityChip by mutableStateOf("")
    var adaptiveBitrateBps by mutableIntStateOf(0)
    var exposureEv by mutableIntStateOf(0)
    var senderTab by mutableStateOf(SenderTab.Devices)
    var phonePairingConfirmed by mutableStateOf(false)
    var discoveryComplete by mutableStateOf(false)
    var wifiPillText by mutableStateOf("局域网")
    var pairingRemainingSeconds by mutableIntStateOf(60)
    var pairingExpired by mutableStateOf(false)
    var pairingStartedAtMs by mutableLongStateOf(0L)
    var waitOutcome by mutableStateOf(WaitOutcome.Pending)
    var reconnectAttempt by mutableIntStateOf(0)
    var reconnectDelayMs by mutableLongStateOf(0L)
    var lastShownSessionError by mutableStateOf("")
}
