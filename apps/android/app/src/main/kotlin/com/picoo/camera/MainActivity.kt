package com.picoo.camera

import android.Manifest
import android.content.pm.PackageManager
import android.os.Bundle
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import com.picoo.camera.discovery.NsdReceiverBrowser
import com.picoo.camera.discovery.PairedAutoConnect
import com.picoo.camera.jni.PicooNative
import com.picoo.camera.media.Camera2MediaEncoder
import com.picoo.camera.media.CaptureState
import com.picoo.camera.media.EncodedFrameListener
import com.picoo.camera.media.LensFacing
import com.picoo.camera.media.LinkQuality
import com.picoo.camera.media.LocalPreviewMirror
import com.picoo.camera.media.MediaBitrate
import com.picoo.camera.media.ParameterSetsListener
import com.picoo.camera.pairing.TrustedDeviceList
import com.picoo.camera.ui.SenderTab
import com.picoo.camera.ui.screens.DevicesScreen
import com.picoo.camera.ui.screens.PairingScreen
import com.picoo.camera.ui.screens.QrScanScreen
import com.picoo.camera.ui.screens.SettingsScreen
import com.picoo.camera.ui.screens.StreamingScreen
import com.picoo.camera.ui.screens.WaitScreen
import com.picoo.camera.ui.theme.PicooCameraTheme
import kotlinx.coroutines.delay
import java.util.concurrent.atomic.AtomicReference

class MainActivity : ComponentActivity() {
    private var cameraGranted by mutableStateOf(false)
    private var nearbyWifiGranted by mutableStateOf(true)
    private var notificationsGranted by mutableStateOf(true)
    private var pendingAfterCameraGrant: (() -> Unit)? = null
    private var activeSenderHandle: Long = 0L

    private val permissionLauncher =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            cameraGranted = granted
            val pending = pendingAfterCameraGrant
            pendingAfterCameraGrant = null
            if (granted) {
                if (activeSenderHandle != 0L && PicooNative.isAvailable) {
                    PicooNative.clearPermissionRequired(activeSenderHandle)
                }
                pending?.invoke()
            } else if (activeSenderHandle != 0L && PicooNative.isAvailable) {
                PicooNative.markPermissionRequired(activeSenderHandle)
            }
        }

    private val nearbyWifiLauncher =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            nearbyWifiGranted = granted
        }

    private val notificationsLauncher =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            notificationsGranted = granted
        }

    fun requestCameraPermission(then: (() -> Unit)? = null) {
        if (ContextCompat.checkSelfPermission(this, Manifest.permission.CAMERA) ==
            PackageManager.PERMISSION_GRANTED
        ) {
            cameraGranted = true
            if (activeSenderHandle != 0L && PicooNative.isAvailable) {
                PicooNative.clearPermissionRequired(activeSenderHandle)
            }
            then?.invoke()
            return
        }
        pendingAfterCameraGrant = then
        permissionLauncher.launch(Manifest.permission.CAMERA)
    }

    fun ensureNearbyWifiPermission() {
        if (android.os.Build.VERSION.SDK_INT < android.os.Build.VERSION_CODES.TIRAMISU) {
            nearbyWifiGranted = true
            return
        }
        val perm = Manifest.permission.NEARBY_WIFI_DEVICES
        if (ContextCompat.checkSelfPermission(this, perm) == PackageManager.PERMISSION_GRANTED) {
            nearbyWifiGranted = true
            return
        }
        nearbyWifiLauncher.launch(perm)
    }

    fun ensureNotificationsPermission() {
        if (android.os.Build.VERSION.SDK_INT < android.os.Build.VERSION_CODES.TIRAMISU) {
            notificationsGranted = true
            return
        }
        val perm = Manifest.permission.POST_NOTIFICATIONS
        if (ContextCompat.checkSelfPermission(this, perm) == PackageManager.PERMISSION_GRANTED) {
            notificationsGranted = true
            return
        }
        notificationsLauncher.launch(perm)
    }

    fun bindSenderHandle(handle: Long) {
        activeSenderHandle = handle
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()

        cameraGranted =
            ContextCompat.checkSelfPermission(this, Manifest.permission.CAMERA) ==
                PackageManager.PERMISSION_GRANTED
        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.TIRAMISU) {
            nearbyWifiGranted =
                ContextCompat.checkSelfPermission(this, Manifest.permission.NEARBY_WIFI_DEVICES) ==
                    PackageManager.PERMISSION_GRANTED
            notificationsGranted =
                ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS) ==
                    PackageManager.PERMISSION_GRANTED
        }

        val nativeOk = PicooNative.ensureLoaded()

        setContent {
            PicooCameraTheme {
                if (!nativeOk) {
                    NativeLoadFailedScreen(
                        detail = PicooNative.loadError ?: "unknown",
                        modifier = Modifier.padding(24.dp),
                    )
                } else {
                    SenderHomeScreen(
                        cameraGranted = cameraGranted,
                        nearbyWifiGranted = nearbyWifiGranted,
                        notificationsGranted = notificationsGranted,
                        onRequestCamera = { then -> requestCameraPermission(then) },
                        onRequestNearbyWifi = { ensureNearbyWifiPermission() },
                        onRequestNotifications = { ensureNotificationsPermission() },
                    )
                }
            }
        }
    }
}

@Composable
private fun NativeLoadFailedScreen(
    detail: String,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier = modifier.fillMaxSize(),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.Start,
    ) {
        Text("无法加载 Picoo 原生库", style = MaterialTheme.typography.headlineSmall)
        Spacer(modifier = Modifier.height(12.dp))
        Text(
            "常见原因：设备使用 16KB 内存页，而旧版 APK 中的 .so 未按 16KB 对齐（小米 15 / Android 15）。请安装最新 CI 构建。",
            style = MaterialTheme.typography.bodyMedium,
        )
        Spacer(modifier = Modifier.height(12.dp))
        Text(detail, style = MaterialTheme.typography.bodySmall)
    }
}

@Composable
private fun SenderHomeScreen(
    cameraGranted: Boolean,
    nearbyWifiGranted: Boolean,
    notificationsGranted: Boolean,
    onRequestCamera: (then: (() -> Unit)?) -> Unit,
    onRequestNearbyWifi: () -> Unit,
    onRequestNotifications: () -> Unit,
) {
    val context = LocalContext.current
    var encoderState by remember { mutableStateOf(CaptureState.Idle) }
    var errorText by remember { mutableStateOf<String?>(null) }
    var hostText by remember { mutableStateOf("127.0.0.1") }
    var portText by remember { mutableStateOf("4433") }
    var senderStatus by remember { mutableIntStateOf(PicooNative.STATUS_DISCONNECTED) }
    var pairingCode by remember { mutableStateOf("") }
    val discoveredListState =
        remember { mutableStateOf<List<PicooNative.DiscoveredReceiver>>(emptyList()) }
    val discoveredList = discoveredListState.value
    var connectedReceiverId by remember { mutableStateOf("") }
    var connectedReceiverName by remember { mutableStateOf("") }
    var pairedDevices by remember { mutableStateOf<List<PicooNative.TrustedDevice>>(emptyList()) }
    var selectedReceiverId by remember { mutableStateOf("") }
    var selectedReceiverName by remember { mutableStateOf("") }
    var trustedStoreHandle by remember { mutableLongStateOf(0L) }
    val pairedReceiverIds = remember { mutableStateOf<Set<String>>(emptySet()) }
    val autoConnectAttemptedIds = remember { mutableStateOf<Set<String>>(emptySet()) }
    var suppressAutoConnect by remember { mutableStateOf(false) }
    var remoteMirrored by remember { mutableStateOf(false) }
    var localPreviewMirrored by remember {
        mutableStateOf(LocalPreviewMirror.defaultFor(LensFacing.Back))
    }
    var resolutionLabel by remember { mutableStateOf("720p") }
    var preferredResolutionLabel by remember { mutableStateOf("1080p") }
    var powerHint by remember { mutableStateOf("") }
    var thermalForced720 by remember { mutableStateOf(false) }
    var linkQualityChip by remember { mutableStateOf("") }
    var adaptiveBitrateBps by remember { mutableIntStateOf(3_000_000) }
    var exposureEv by remember { mutableIntStateOf(0) }
    var senderTab by remember { mutableStateOf(SenderTab.Devices) }
    var phonePairingConfirmed by remember { mutableStateOf(false) }
    var viaQr by remember { mutableStateOf(false) }
    var qrSuccessName by remember { mutableStateOf<String?>(null) }
    var qrHostPort by remember { mutableStateOf<String?>(null) }
    var discoveryComplete by remember { mutableStateOf(false) }
    val parameterSetsRef = remember { AtomicReference<Pair<ByteArray, ByteArray>?>(null) }
    val streamConfigDirty = remember { java.util.concurrent.atomic.AtomicBoolean(false) }

    val trustedStorePath = remember {
        java.io.File(context.filesDir, "trusted_devices.json").absolutePath
    }
    val identityPath = remember {
        java.io.File(context.filesDir, "sender_identity.json").absolutePath
    }
    val identityHandle = remember {
        PicooNative.loadOrCreateIdentity(identityPath, android.os.Build.MODEL)
    }
    val senderDeviceId = remember(identityHandle) {
        if (identityHandle == 0L) "android-sender" else PicooNative.getIdentityDeviceId(identityHandle)
    }
    val senderPublicKey = remember(identityHandle) {
        if (identityHandle == 0L) byteArrayOf(1, 2, 3) else PicooNative.getIdentityPublicKey(identityHandle)
    }
    val senderDeviceName = remember(identityHandle) {
        if (identityHandle == 0L) {
            android.os.Build.MODEL
        } else {
            PicooNative.getIdentityDeviceName(identityHandle).ifBlank { android.os.Build.MODEL }
        }
    }
    val senderHandle = remember { PicooNative.createSender() }
    LaunchedEffect(senderHandle) {
        (context as? MainActivity)?.bindSenderHandle(senderHandle)
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

    LaunchedEffect(discoveredList) {
        if (discoveredList.isEmpty()) {
            discoveryComplete = false
            delay(3_000)
            if (discoveredListState.value.isEmpty()) {
                discoveryComplete = true
            }
        } else {
            discoveryComplete = false
        }
    }

    DisposableEffect(nsdBrowser, nearbyWifiGranted) {
        if (nearbyWifiGranted) nsdBrowser.start()
        onDispose { nsdBrowser.stop() }
    }

    val encoder = remember {
        Camera2MediaEncoder(
            context = context,
            frameListener = EncodedFrameListener { data, isKeyFrame, ptsUs, streamEpoch ->
                val fragments = PicooNative.ingestAccessUnit(
                    handle = senderHandle,
                    data = data,
                    keyframe = isKeyFrame,
                    ptsUs = ptsUs,
                    streamEpoch = streamEpoch,
                )
                if (fragments > 0) {
                    PicooNative.flushPending(senderHandle)
                    PicooNative.pump(senderHandle)
                }
            },
            parameterSetsListener = ParameterSetsListener { sps, pps ->
                parameterSetsRef.set(sps to pps)
                streamConfigDirty.set(true)
            },
        )
    }

    fun applyStreamConfigToSender() {
        val (width, height) = if (resolutionLabel == "1080p") 1920 to 1080 else 1280 to 720
        val bitrate = when {
            adaptiveBitrateBps > 0 -> adaptiveBitrateBps
            else -> MediaBitrate.forResolution(width, height)
        }
        val sets = parameterSetsRef.get()
        PicooNative.setStreamConfig(
            senderHandle,
            width = width,
            height = height,
            fps = 30,
            bitrateBps = bitrate,
            streamEpoch = encoder.streamEpoch,
            mirrored = remoteMirrored,
            rotation = encoder.sensorOrientationDegrees(),
            sps = sets?.first,
            pps = sets?.second,
        )
        streamConfigDirty.set(false)
    }

    fun connectToReceiver(host: String, port: Int, receiverId: String, qrNonce: String = "") {
        suppressAutoConnect = false
        applyStreamConfigToSender()
        val rc = PicooNative.connect(senderHandle, host.trim(), port)
        if (rc == 0) {
            selectedReceiverId = receiverId
            PicooNative.sendClientHello(
                senderHandle,
                senderId = senderDeviceId,
                deviceName = senderDeviceName,
                publicKey = senderPublicKey,
                qrNonce = qrNonce,
            )
            errorText = null
        } else {
            errorText = "连接失败 ($rc)"
        }
    }

    fun connectFromQrJson(json: String): Boolean {
        val payload = PicooNative.parseQrConnect(json.trim())
        if (payload == null) {
            errorText = "二维码无效或已过期"
            return false
        }
        hostText = payload.host
        portText = payload.quicPort.toString()
        selectedReceiverId = payload.receiverId
        selectedReceiverName = payload.receiverId
        qrHostPort = "${payload.host}:${payload.quicPort}"
        connectToReceiver(payload.host, payload.quicPort, payload.receiverId, payload.nonce)
        return true
    }

    fun reloadTrustedStore() {
        if (trustedStoreHandle != 0L) {
            PicooNative.destroyTrustedStore(trustedStoreHandle)
        }
        trustedStoreHandle = PicooNative.loadTrustedStore(trustedStorePath)
        val devices = TrustedDeviceList.load(trustedStoreHandle)
        pairedDevices = devices
        pairedReceiverIds.value = TrustedDeviceList.ids(devices)
    }

    fun resetToDevices() {
        senderTab = SenderTab.Devices
        phonePairingConfirmed = false
        viaQr = false
        qrSuccessName = null
        qrHostPort = null
        errorText = null
    }

    LaunchedEffect(senderHandle, trustedStorePath) {
        if (senderHandle != 0L) {
            PicooNative.attachTrustedStore(senderHandle, trustedStorePath)
        }
        reloadTrustedStore()
    }

    LaunchedEffect(discoveredList, pairedReceiverIds.value, senderStatus, suppressAutoConnect) {
        if (suppressAutoConnect) return@LaunchedEffect
        val sessionBusy = when (senderStatus) {
            PicooNative.STATUS_DISCONNECTED,
            PicooNative.STATUS_DISCOVERING,
            -> false
            else -> true
        }
        val pick = PairedAutoConnect.pick(
            discovered = discoveredList,
            pairedReceiverIds = pairedReceiverIds.value,
            sessionBusy = sessionBusy,
            alreadyAttemptedIds = autoConnectAttemptedIds.value,
        ) ?: return@LaunchedEffect
        autoConnectAttemptedIds.value = autoConnectAttemptedIds.value + pick.receiverId
        hostText = pick.host
        portText = pick.quicPort.toString()
        selectedReceiverId = pick.receiverId
        selectedReceiverName = pick.displayName
        connectToReceiver(pick.host, pick.quicPort, pick.receiverId)
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
            PicooNative.STATUS_NEGOTIATING,
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
        val shouldEncode = cameraGranted && when (senderStatus) {
            PicooNative.STATUS_STREAMING,
            PicooNative.STATUS_NEGOTIATING,
            PicooNative.STATUS_RECONNECTING,
            PicooNative.STATUS_NETWORK_UNSTABLE,
            -> true
            else -> false
        }
        if (shouldEncode) {
            onRequestCamera(null)
            encoder.startPreview()
            if (senderHandle != 0L && PicooNative.takeKeyframeRequest(senderHandle) == 1) {
                encoder.requestKeyFrame()
            }
            encoderState = encoder.state
        } else if (senderStatus == PicooNative.STATUS_DISCONNECTED &&
            encoder.state == CaptureState.Previewing
        ) {
            encoder.stopPreview()
            encoderState = encoder.state
        }
    }

    LaunchedEffect(senderStatus) {
        val live = when (senderStatus) {
            PicooNative.STATUS_STREAMING,
            PicooNative.STATUS_NEGOTIATING,
            PicooNative.STATUS_RECONNECTING,
            PicooNative.STATUS_NETWORK_UNSTABLE,
            -> true
            else -> false
        }
        if (!live) {
            powerHint = ""
            thermalForced720 = false
            return@LaunchedEffect
        }
        while (true) {
            val thermal = PowerHints.readThermalStatus(context)
            powerHint = PowerHints.readHint(context)
            val force720 = PowerHints.shouldForce720p(thermal)
            thermalForced720 = force720
            if (senderHandle != 0L) {
                PicooNative.setThermalHold(senderHandle, force720)
            }
            if (force720 && resolutionLabel != "720p") {
                resolutionLabel = "720p"
                encoder.setResolution(1280, 720)
                if (senderHandle != 0L) {
                    PicooNative.syncEncodeHeight(senderHandle, 720)
                }
                streamConfigDirty.set(true)
                encoder.requestKeyFrame()
            }
            delay(5_000)
        }
    }

    LaunchedEffect(senderHandle) {
        var previousStatus = PicooNative.STATUS_DISCONNECTED
        while (true) {
            if (senderHandle != 0L) {
                PicooNative.pump(senderHandle)
                senderStatus = PicooNative.getSenderStatus(senderHandle)
                encoderState = encoder.state
                pairingCode = PicooNative.getPairingShortCode(senderHandle)
                connectedReceiverId = PicooNative.getConnectedReceiverId(senderHandle)
                connectedReceiverName =
                    PicooNative.getConnectedReceiverDisplayName(senderHandle)
                if (connectedReceiverName.isNotEmpty()) {
                    selectedReceiverName = connectedReceiverName
                }
                val bps = PicooNative.getCurrentBitrateBps(senderHandle)
                if (bps > 0) {
                    adaptiveBitrateBps = bps
                    encoder.setTargetBitrateBps(bps)
                }
                val link = PicooNative.getLinkStats(senderHandle)
                linkQualityChip = if (link != null && link.size >= 6) {
                    LinkQuality.formatChip(rttMs = link[0], packetLoss = link[1])
                } else {
                    ""
                }
                if (PicooNative.takeKeyframeRequest(senderHandle) == 1) {
                    encoder.requestKeyFrame()
                }
                run {
                    val camOut = IntArray(3)
                    when (PicooNative.takeCameraCommand(senderHandle, camOut)) {
                        1 -> {
                            encoder.setLensFacing(LensFacing.Front)
                            localPreviewMirrored =
                                LocalPreviewMirror.defaultFor(encoder.profile.lensFacing)
                            streamConfigDirty.set(true)
                        }
                        2 -> {
                            encoder.setLensFacing(LensFacing.Back)
                            localPreviewMirrored =
                                LocalPreviewMirror.defaultFor(encoder.profile.lensFacing)
                            streamConfigDirty.set(true)
                        }
                        3 -> {
                            val w = camOut[0]
                            val h = camOut[1]
                            if (w > 0 && h > 0) {
                                resolutionLabel = if (h >= 1080) "1080p" else "720p"
                                preferredResolutionLabel = resolutionLabel
                                encoder.setResolution(w, h)
                                PicooNative.setPreferredHeight(senderHandle, h)
                                PicooNative.syncEncodeHeight(senderHandle, h)
                                streamConfigDirty.set(true)
                                encoder.requestKeyFrame()
                            }
                        }
                        4 -> {
                            remoteMirrored = camOut[2] != 0
                            streamConfigDirty.set(true)
                        }
                    }
                }
                if (PicooNative.takeResolutionDownshift(senderHandle) == 1 &&
                    resolutionLabel != "720p"
                ) {
                    resolutionLabel = "720p"
                    encoder.setResolution(1280, 720)
                    streamConfigDirty.set(true)
                    encoder.requestKeyFrame()
                }
                if (!PowerHints.shouldForce720p(PowerHints.readThermalStatus(context)) &&
                    PicooNative.takeResolutionUpshift(senderHandle) == 1 &&
                    preferredResolutionLabel == "1080p" &&
                    resolutionLabel != "1080p"
                ) {
                    val maxH = PicooNative.getReceiverMaxHeight(senderHandle)
                    if (maxH == 0 || maxH >= 1080) {
                        resolutionLabel = "1080p"
                        encoder.setResolution(1920, 1080)
                        PicooNative.syncEncodeHeight(senderHandle, 1080)
                        streamConfigDirty.set(true)
                        encoder.requestKeyFrame()
                    }
                }
                if (previousStatus == PicooNative.STATUS_RECONNECTING &&
                    senderStatus == PicooNative.STATUS_STREAMING
                ) {
                    encoder.requestKeyFrame()
                    streamConfigDirty.set(true)
                }
                if (senderStatus == PicooNative.STATUS_DISCONNECTED &&
                    previousStatus != PicooNative.STATUS_DISCONNECTED &&
                    !suppressAutoConnect
                ) {
                    autoConnectAttemptedIds.value = emptySet()
                }
                previousStatus = senderStatus
                if (streamConfigDirty.get() &&
                    (senderStatus == PicooNative.STATUS_STREAMING ||
                        senderStatus == PicooNative.STATUS_NEGOTIATING)
                ) {
                    applyStreamConfigToSender()
                }
            }
            delay(500)
        }
    }

    LaunchedEffect(senderStatus, pairingCode, qrSuccessName, phonePairingConfirmed, senderTab) {
        when {
            senderTab == SenderTab.Settings -> Unit
            senderStatus == PicooNative.STATUS_STREAMING ||
                senderStatus == PicooNative.STATUS_NEGOTIATING ||
                senderStatus == PicooNative.STATUS_RECONNECTING ||
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
                !phonePairingConfirmed &&
                (qrSuccessName != null || senderTab != SenderTab.Qr) ->
                senderTab = SenderTab.Pairing
        }
    }

    DisposableEffect(encoder, senderHandle, trustedStoreHandle, identityHandle) {
        onDispose {
            encoder.close()
            if (trustedStoreHandle != 0L) {
                PicooNative.destroyTrustedStore(trustedStoreHandle)
            }
            if (senderHandle != 0L) {
                PicooNative.destroySender(senderHandle)
            }
            if (identityHandle != 0L) {
                PicooNative.destroyIdentity(identityHandle)
            }
        }
    }

    val bitrateMbps = "%.1f Mbps".format(adaptiveBitrateBps / 1_000_000.0)
    val pairingDisplayName = selectedReceiverName.ifBlank { connectedReceiverName }

    Box(modifier = Modifier.fillMaxSize()) {
        when (senderTab) {
            SenderTab.Devices -> DevicesScreen(
                discoveredList = discoveredList,
                pairedDevices = pairedDevices,
                pairedReceiverIds = pairedReceiverIds.value,
                nearbyWifiGranted = nearbyWifiGranted,
                discoveryComplete = discoveryComplete,
                errorText = errorText,
                onSelectReceiver = { receiver ->
                    selectedReceiverName = receiver.displayName
                    hostText = receiver.host
                    portText = receiver.quicPort.toString()
                    selectedReceiverId = receiver.receiverId
                    viaQr = false
                    connectToReceiver(receiver.host, receiver.quicPort, receiver.receiverId)
                },
                onScanQr = {
                    viaQr = true
                    onRequestCamera { senderTab = SenderTab.Qr }
                },
                onCheckPermissions = {
                    onRequestNearbyWifi()
                    onRequestNotifications()
                    onRequestCamera(null)
                },
                onRemovePaired = { device ->
                    selectedReceiverId = device.deviceId
                    val rc = PicooNative.removeTrustedDevice(trustedStoreHandle, device.deviceId)
                    if (rc == 1) {
                        PicooNative.saveTrustedStore(trustedStoreHandle)
                        PicooNative.attachTrustedStore(senderHandle, trustedStorePath)
                        autoConnectAttemptedIds.value =
                            autoConnectAttemptedIds.value - device.deviceId
                        reloadTrustedStore()
                        errorText = null
                    } else {
                        errorText = "删除失败 ($rc)"
                    }
                },
                onRequestNearbyWifi = onRequestNearbyWifi,
                onOpenSettings = { senderTab = SenderTab.Settings },
            )
            SenderTab.Settings -> SettingsScreen(
                pairedDeviceCount = pairedDevices.size,
                cameraGranted = cameraGranted,
                nearbyWifiGranted = nearbyWifiGranted,
                notificationsGranted = notificationsGranted,
                onBack = { senderTab = SenderTab.Devices },
                onCheckPermissions = {
                    onRequestNearbyWifi()
                    onRequestNotifications()
                    onRequestCamera(null)
                },
                onOpenPairedDevices = { senderTab = SenderTab.Devices },
            )
            SenderTab.Qr -> QrScanScreen(
                cameraGranted = cameraGranted,
                qrSuccessName = qrSuccessName,
                qrHostPort = qrHostPort,
                errorText = errorText,
                onBack = { resetToDevices() },
                onQrDetected = { payload ->
                    if (connectFromQrJson(payload)) {
                        val name = discoveredList.find {
                            it.receiverId == selectedReceiverId
                        }?.displayName ?: selectedReceiverId
                        qrSuccessName = name
                        selectedReceiverName = name
                    }
                },
                onContinuePairing = { senderTab = SenderTab.Pairing },
                onRequestCamera = { onRequestCamera(null) },
                onManualConnect = { host, port ->
                    hostText = host
                    portText = port.toString()
                    selectedReceiverId = "manual-$host"
                    selectedReceiverName = host
                    viaQr = true
                    qrHostPort = "$host:$port"
                    connectToReceiver(host, port, selectedReceiverId)
                },
            )
            SenderTab.Pairing -> PairingScreen(
                receiverName = pairingDisplayName,
                pairingCode = pairingCode,
                viaQr = viaQr,
                errorText = errorText,
                onConfirm = {
                    val receiverId =
                        connectedReceiverId.ifEmpty {
                            selectedReceiverId.ifEmpty { "windows-receiver" }
                        }
                    val rc = PicooNative.sendPairingConfirm(senderHandle, receiverId)
                    if (rc == 0) {
                        errorText = null
                        reloadTrustedStore()
                        phonePairingConfirmed = true
                        senderTab = SenderTab.Wait
                    } else {
                        errorText = "配对确认失败 ($rc)"
                    }
                },
                onCancel = {
                    PicooNative.disconnect(senderHandle)
                    pairingCode = ""
                    connectedReceiverId = ""
                    connectedReceiverName = ""
                    phonePairingConfirmed = false
                    suppressAutoConnect = true
                    resetToDevices()
                },
            )
            SenderTab.Wait -> WaitScreen(
                receiverName = pairingDisplayName,
                onCancel = {
                    PicooNative.disconnect(senderHandle)
                    phonePairingConfirmed = false
                    suppressAutoConnect = true
                    resetToDevices()
                },
            )
            SenderTab.Streaming -> StreamingScreen(
                cameraGranted = cameraGranted,
                receiverName = pairingDisplayName,
                linkQualityChip = linkQualityChip,
                resolutionLabel = resolutionLabel,
                bitrateMbps = bitrateMbps,
                lensFacing = encoder.profile.lensFacing,
                localPreviewMirrored = localPreviewMirrored,
                thermalForced720 = thermalForced720,
                powerHint = if (thermalForced720) "" else powerHint,
                onRequestCamera = { onRequestCamera(null) },
                onFlipCamera = {
                    encoder.switchCamera()
                    localPreviewMirrored =
                        LocalPreviewMirror.defaultFor(encoder.profile.lensFacing)
                    encoderState = encoder.state
                    applyStreamConfigToSender()
                },
                onToggleResolution = {
                    val maxH = PicooNative.getReceiverMaxHeight(senderHandle)
                    val want1080 = resolutionLabel == "720p"
                    if (want1080 && maxH in 1 until 1080) {
                        errorText = "接收端最高 ${maxH}p — 保持 720p"
                        return@StreamingScreen
                    }
                    if (thermalForced720 && want1080) {
                        errorText = "设备过热，暂不可升 1080p"
                        return@StreamingScreen
                    }
                    resolutionLabel = if (want1080) "1080p" else "720p"
                    preferredResolutionLabel = resolutionLabel
                    val (w, h) = if (resolutionLabel == "1080p") 1920 to 1080 else 1280 to 720
                    PicooNative.setPreferredHeight(senderHandle, h)
                    PicooNative.syncEncodeHeight(senderHandle, h)
                    encoder.setResolution(w, h)
                    encoderState = encoder.state
                    applyStreamConfigToSender()
                    errorText = null
                },
                onToggleMirror = { localPreviewMirrored = !localPreviewMirrored },
                onEvStep = {
                    val range = encoder.exposureCompensationRange
                    if (range.isEmpty()) return@StreamingScreen
                    val next = if (exposureEv >= 2) -2 else exposureEv + 1
                    encoder.setExposureCompensation(next)
                    exposureEv = encoder.exposureCompensation
                },
                exposureEv = exposureEv,
                evSupported = !encoder.exposureCompensationRange.isEmpty(),
                onDisconnect = {
                    suppressAutoConnect = true
                    PicooNative.disconnect(senderHandle)
                    senderStatus = PicooNative.getSenderStatus(senderHandle)
                    resetToDevices()
                },
                onPreviewSurfaceAvailable = { surface ->
                    encoder.bindPreviewSurface(surface)
                    encoderState = encoder.state
                    errorText = encoder.lastError
                },
                onPreviewSurfaceDestroyed = { encoder.unbindPreviewSurface() },
            )
        }
    }
}
