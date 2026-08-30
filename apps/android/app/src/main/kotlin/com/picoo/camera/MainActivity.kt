package com.picoo.camera

import android.Manifest
import android.content.pm.PackageManager
import android.os.Bundle
import android.view.WindowManager
import android.widget.Toast
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
import com.picoo.camera.media.EncoderReconfigurationCoordinator
import com.picoo.camera.media.LensFacing
import com.picoo.camera.media.LinkQuality
import com.picoo.camera.media.LocalPreviewMirror
import com.picoo.camera.media.ParameterSetsListener
import com.picoo.camera.media.StreamResolution
import com.picoo.camera.pairing.TrustedDeviceList
import com.picoo.camera.runtime.SenderNativeRuntime
import com.picoo.camera.ui.SenderTab
import com.picoo.camera.ui.SenderHomeState
import com.picoo.camera.network.WifiNetworkInfo
import com.picoo.camera.ui.screens.DevicesScreen
import com.picoo.camera.ui.screens.ManualConnectScreen
import com.picoo.camera.ui.screens.PairingScreen
import com.picoo.camera.ui.screens.SettingsScreen
import com.picoo.camera.ui.screens.StreamingScreen
import com.picoo.camera.ui.screens.WaitOutcome
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
    val uiState = remember { SenderHomeState() }
    var encoderState by uiState::encoderState
    var errorText by uiState::errorText
    var hostText by uiState::hostText
    var portText by uiState::portText
    var senderStatus by uiState::senderStatus
    var pairingCode by uiState::pairingCode
    val discoveredListState = uiState.discoveredReceivers
    val discoveredList = discoveredListState.value
    var connectedReceiverId by uiState::connectedReceiverId
    var connectedReceiverName by uiState::connectedReceiverName
    var pairedDevices by uiState::pairedDevices
    var selectedReceiverId by uiState::selectedReceiverId
    var selectedReceiverName by uiState::selectedReceiverName
    var suppressAutoConnect by uiState::suppressAutoConnect
    var remoteMirrored by uiState::remoteMirrored
    var localPreviewMirrored by uiState::localPreviewMirrored
    var resolutionLabel by uiState::resolutionLabel
    var preferredResolutionLabel by uiState::preferredResolutionLabel
    var powerHint by uiState::powerHint
    var thermalForced720 by uiState::thermalForced720
    var thermalToastShown by uiState::thermalToastShown
    var linkQualityChip by uiState::linkQualityChip
    var adaptiveBitrateBps by uiState::adaptiveBitrateBps
    var exposureEv by uiState::exposureEv
    var senderTab by uiState::senderTab
    var phonePairingConfirmed by uiState::phonePairingConfirmed
    var discoveryComplete by uiState::discoveryComplete
    var wifiPillText by uiState::wifiPillText
    var pairingRemainingSeconds by uiState::pairingRemainingSeconds
    var pairingExpired by uiState::pairingExpired
    var pairingStartedAtMs by uiState::pairingStartedAtMs
    var waitOutcome by uiState::waitOutcome
    var waitUserCancelled by uiState::waitUserCancelled
    var reconnectAttempt by uiState::reconnectAttempt
    var reconnectDelayMs by uiState::reconnectDelayMs
    var lastShownSessionError by uiState::lastShownSessionError
    val parameterSetsRef = remember { AtomicReference<Pair<ByteArray, ByteArray>?>(null) }
    val streamConfigDirty = remember { java.util.concurrent.atomic.AtomicBoolean(false) }
    val runtime = remember(context) { SenderNativeRuntime(context) }
    val senderDeviceId = remember(runtime) { runtime.senderDeviceId }
    val senderPublicKey = remember(runtime) { runtime.senderPublicKey }
    val senderDeviceName = remember(runtime) { runtime.senderDeviceName }
    val senderHandle = runtime.senderHandle
    val pairedReceiverIds = TrustedDeviceList.ids(pairedDevices)
    LaunchedEffect(senderHandle) {
        (context as? MainActivity)?.bindSenderHandle(senderHandle)
        if (senderHandle != 0L) {
            adaptiveBitrateBps = PicooNative.readSenderSnapshot(senderHandle).currentBitrateBps
        }
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
        waitUserCancelled = false
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

    DisposableEffect(nsdBrowser, nearbyWifiGranted) {
        if (nearbyWifiGranted) nsdBrowser.start()
        onDispose { nsdBrowser.stop() }
    }

    val encoder = remember {
        Camera2MediaEncoder(
            context = context,
            initialBitrateBps = PicooNative.bitrateInitialForHeight(StreamResolution.P720.height),
            initialStreamEpoch = PicooNative.readSenderSnapshot(senderHandle).streamEpoch,
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
    var previewTransformInfo by remember { mutableStateOf(encoder.previewTransformInfo) }
    val encoderReconfiguration = remember { EncoderReconfigurationCoordinator() }

    fun applyStreamConfigToSender() {
        val width = encoder.profile.resolution.width
        val height = encoder.profile.resolution.height
        val rustBitrate = PicooNative.readSenderSnapshot(senderHandle).currentBitrateBps
        val bitrate = if (rustBitrate > 0) {
            rustBitrate
        } else {
            PicooNative.bitrateInitialForHeight(height)
        }
        val sets = parameterSetsRef.get()
        PicooNative.setStreamConfig(
            senderHandle,
            width = width,
            height = height,
            fps = 30,
            bitrateBps = bitrate,
            mirrored = remoteMirrored,
            rotation = encoder.sensorOrientationDegrees(),
            sps = sets?.first,
            pps = sets?.second,
        )
        streamConfigDirty.set(false)
    }

    fun beginLocalEncoderReconfiguration(targetHeight: Int): Boolean {
        val epoch = encoderReconfiguration.beginLocal(senderHandle, encoder, targetHeight)
        if (epoch == 0) {
            errorText = "正在完成上一项视频调整，请稍后重试"
            return false
        }
        return true
    }

    fun connectToReceiver(host: String, port: Int, receiverId: String) {
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
            )
            errorText = null
        } else {
            errorText = "连接失败 ($rc)"
        }
    }

    fun reloadTrustedStore(): Boolean {
        val devices = runtime.reloadTrustedDevices() ?: run {
            errorText = "无法读取已配对设备，已保留上一次有效数据"
            return false
        }
        pairedDevices = devices
        return true
    }

    fun resetToDevices() {
        senderTab = SenderTab.Devices
        phonePairingConfirmed = false
        pairingExpired = false
        pairingRemainingSeconds = 60
        waitOutcome = WaitOutcome.Pending
        waitUserCancelled = false
        errorText = null
    }

    fun regeneratePairing() {
        pairingExpired = false
        pairingRemainingSeconds = 60
        phonePairingConfirmed = false
        waitOutcome = WaitOutcome.Pending
        PicooNative.disconnect(senderHandle)
        val port = portText.toIntOrNull() ?: 4433
        connectToReceiver(hostText, port, selectedReceiverId)
    }

    LaunchedEffect(runtime) {
        if (runtime.attachTrustedStore() != 0) {
            errorText = "无法加载配对存储"
        }
        reloadTrustedStore()
    }

    LaunchedEffect(discoveredList, pairedReceiverIds, senderStatus, suppressAutoConnect) {
        if (suppressAutoConnect) return@LaunchedEffect
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
        } else if (encoder.state == CaptureState.Opening ||
            encoder.state == CaptureState.Previewing
        ) {
            encoder.stopPreview()
            encoderState = encoder.state
        }
    }

    LaunchedEffect(senderStatus) {
        val live = when (senderStatus) {
            PicooNative.STATUS_STREAMING,
            PicooNative.STATUS_RECONNECTING,
            PicooNative.STATUS_NETWORK_UNSTABLE,
            -> true
            else -> false
        }
        if (!live) {
            powerHint = ""
            thermalForced720 = false
            thermalToastShown = false
            return@LaunchedEffect
        }
        while (true) {
            val thermal = PowerHints.readThermalStatus(context)
            powerHint = PowerHints.readHint(context)
            val force720 = PowerHints.shouldForce720p(thermal)
            thermalForced720 = force720
            if (force720 && !thermalToastShown) {
                // AC-M-LIVE-02: toast when thermal throttle engages (banner remains visible).
                Toast.makeText(
                    context,
                    "设备偏热保护中 · 已降至 720p，1080P 暂不可选",
                    Toast.LENGTH_SHORT,
                ).show()
                thermalToastShown = true
            } else if (!force720) {
                thermalToastShown = false
            }
            if (senderHandle != 0L) {
                PicooNative.setThermalHold(senderHandle, force720)
            }
            if (force720 && resolutionLabel == "1080p") {
                val targetBitrate = PicooNative.bitrateInitialForHeight(720)
                if (beginLocalEncoderReconfiguration(720)) {
                    resolutionLabel = "720p"
                    encoder.setTargetBitrateBps(targetBitrate)
                    encoder.setResolution(1280, 720)
                    streamConfigDirty.set(true)
                    encoder.requestKeyFrame()
                }
            }
            delay(5_000)
        }
    }

    LaunchedEffect(senderHandle) {
        var previousStatus = PicooNative.STATUS_DISCONNECTED
        while (true) {
            if (senderHandle != 0L) {
                PicooNative.pump(senderHandle)
                var senderSnapshot = PicooNative.readSenderSnapshot(senderHandle)
                senderStatus = senderSnapshot.status
                if (senderStatus == PicooNative.STATUS_RECONNECTING) {
                    reconnectAttempt = senderSnapshot.reconnectAttempt
                    reconnectDelayMs = senderSnapshot.reconnectDelayMs
                } else {
                    reconnectAttempt = 0
                    reconnectDelayMs = 0L
                }
                encoderState = encoder.state
                val latestPreviewTransformInfo = encoder.previewTransformInfo
                if (latestPreviewTransformInfo != previewTransformInfo) {
                    previewTransformInfo = latestPreviewTransformInfo
                }
                if (senderStatus == PicooNative.STATUS_DISCONNECTED) {
                    encoderReconfiguration.abandonDisconnectedSession(senderHandle)
                }
                when (val result = encoderReconfiguration.poll(senderHandle, encoder)) {
                    is EncoderReconfigurationCoordinator.PollResult.Failed -> {
                        streamConfigDirty.set(false)
                        senderSnapshot = PicooNative.readSenderSnapshot(senderHandle)
                        resolutionLabel =
                            StreamResolution.fromHeight(senderSnapshot.activeHeight).label
                        errorText = result.message
                    }
                    is EncoderReconfigurationCoordinator.PollResult.Applied -> {
                        senderSnapshot = PicooNative.readSenderSnapshot(senderHandle)
                        adaptiveBitrateBps = result.bitrateBps
                        resolutionLabel = StreamResolution.fromHeight(result.actualHeight).label
                        encoder.setTargetBitrateBps(adaptiveBitrateBps)
                        streamConfigDirty.set(true)
                        encoder.requestKeyFrame()
                    }
                    is EncoderReconfigurationCoordinator.PollResult.Recovered -> {
                        senderSnapshot = PicooNative.readSenderSnapshot(senderHandle)
                        adaptiveBitrateBps = result.bitrateBps
                        resolutionLabel = StreamResolution.fromHeight(result.actualHeight).label
                        encoder.setTargetBitrateBps(adaptiveBitrateBps)
                        streamConfigDirty.set(true)
                        encoder.requestKeyFrame()
                        errorText = "${result.message}；已恢复上一视频配置"
                    }
                    null -> Unit
                }
                if (!encoderReconfiguration.isPending &&
                    encoder.appliedStreamEpoch == senderSnapshot.streamEpoch &&
                    encoder.appliedEncoderHeight > 0 &&
                    encoder.appliedEncoderHeight != senderSnapshot.activeHeight
                ) {
                    val actualHeight = encoder.appliedEncoderHeight
                    val reportResult = PicooNative.reportEncoderHeight(
                            senderHandle,
                            actualHeight,
                            encoder.appliedStreamEpoch,
                        )
                    if (reportResult == 0) {
                        senderSnapshot = PicooNative.readSenderSnapshot(senderHandle)
                        resolutionLabel = StreamResolution.fromHeight(actualHeight).label
                        streamConfigDirty.set(true)
                    } else if (beginLocalEncoderReconfiguration(actualHeight)) {
                        val actualSize = encoder.profile.resolution
                        encoder.setResolution(actualSize.width, actualSize.height)
                    }
                }
                pairingCode = PicooNative.getPairingShortCode(senderHandle)
                connectedReceiverId = PicooNative.getConnectedReceiverId(senderHandle)
                connectedReceiverName =
                    PicooNative.getConnectedReceiverDisplayName(senderHandle)
                if (connectedReceiverName.isNotEmpty()) {
                    selectedReceiverName = connectedReceiverName
                }
                val bps = senderSnapshot.currentBitrateBps
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
                // PUC-007 / PAIRING-004: surface PUBLIC_KEY_CHANGED / UNPAIRED to the user.
                val sessionErr = PicooNative.lastSessionError(senderHandle)
                if (sessionErr.isNotEmpty() && sessionErr != lastShownSessionError) {
                    lastShownSessionError = sessionErr
                    val msg = when (sessionErr) {
                        "PUBLIC_KEY_CHANGED" ->
                            "电脑端检测到公钥变化，已拒绝自动连接。请删除配对后重新核对短码。"
                        "UNPAIRED" ->
                            "尚未完成配对，无法开始推流。请先完成六位短码确认。"
                        else -> "会话错误：$sessionErr"
                    }
                    errorText = msg
                    Toast.makeText(context, msg, Toast.LENGTH_LONG).show()
                    if (sessionErr == "PUBLIC_KEY_CHANGED") {
                        senderTab = SenderTab.Devices
                    }
                }
                run {
                    val camOut = IntArray(3)
                    when (PicooNative.takeCameraCommand(senderHandle, camOut)) {
                        1 -> {
                            if (encoder.profile.lensFacing != LensFacing.Front &&
                                beginLocalEncoderReconfiguration(encoder.profile.resolution.height)
                            ) {
                                encoder.setLensFacing(LensFacing.Front)
                                localPreviewMirrored =
                                    LocalPreviewMirror.defaultFor(encoder.profile.lensFacing)
                                streamConfigDirty.set(true)
                            }
                        }
                        2 -> {
                            if (encoder.profile.lensFacing != LensFacing.Back &&
                                beginLocalEncoderReconfiguration(encoder.profile.resolution.height)
                            ) {
                                encoder.setLensFacing(LensFacing.Back)
                                localPreviewMirrored =
                                    LocalPreviewMirror.defaultFor(encoder.profile.lensFacing)
                                streamConfigDirty.set(true)
                            }
                        }
                        3 -> {
                            val w = camOut[0]
                            val h = camOut[1]
                            if (w > 0 && h > 0) {
                                val res = StreamResolution.fromHeight(h)
                                if (beginLocalEncoderReconfiguration(res.height)) {
                                    resolutionLabel = res.label
                                    preferredResolutionLabel = res.label
                                    val bitrate = PicooNative.bitrateInitialForHeight(res.height)
                                    encoder.setTargetBitrateBps(bitrate)
                                    encoder.setResolution(res.width, res.height)
                                    PicooNative.setPreferredHeight(senderHandle, res.height)
                                    streamConfigDirty.set(true)
                                    encoder.requestKeyFrame()
                                }
                            }
                        }
                        4 -> {
                            remoteMirrored = camOut[2] != 0
                            streamConfigDirty.set(true)
                        }
                    }
                }
                val receiverMaxHeight = senderSnapshot.receiverMaxHeight
                if (!encoderReconfiguration.isPending &&
                    receiverMaxHeight in 1 until encoder.profile.resolution.height
                ) {
                    val target = StreamResolution.fromHeight(receiverMaxHeight)
                    val targetBitrate = PicooNative.bitrateInitialForHeight(target.height)
                    if (beginLocalEncoderReconfiguration(target.height)) {
                        resolutionLabel = target.label
                        preferredResolutionLabel = target.label
                        encoder.setTargetBitrateBps(targetBitrate)
                        encoder.setResolution(target.width, target.height)
                        streamConfigDirty.set(true)
                        encoder.requestKeyFrame()
                    }
                }
                if (!encoderReconfiguration.isPending) {
                    val directive = PicooNative.readEncoderDirective(senderHandle)
                    if (directive != null) {
                        val maxH = senderSnapshot.receiverMaxHeight
                        val thermalBlocks = thermalForced720 && directive.targetHeight > 720
                        val capabilityBlocks = maxH in 1 until directive.targetHeight
                        if (thermalBlocks || capabilityBlocks) {
                            PicooNative.nackEncoderDirective(senderHandle, directive.id)
                        } else {
                            val next = StreamResolution.fromHeight(directive.targetHeight)
                            if (encoderReconfiguration.beginDirective(
                                    senderHandle,
                                    encoder,
                                    directive,
                                )
                            ) {
                                resolutionLabel = next.label
                                encoder.setTargetBitrateBps(directive.targetBitrateBps)
                                encoder.setResolution(next.width, next.height)
                            }
                        }
                    }
                }
                if (previousStatus == PicooNative.STATUS_RECONNECTING &&
                    senderStatus == PicooNative.STATUS_STREAMING
                ) {
                    if (beginLocalEncoderReconfiguration(encoder.profile.resolution.height)) {
                        encoder.stopPreview()
                        encoder.startPreview()
                        streamConfigDirty.set(true)
                    }
                }
                if (senderTab == SenderTab.Wait &&
                    phonePairingConfirmed &&
                    !waitUserCancelled &&
                    senderStatus == PicooNative.STATUS_DISCONNECTED &&
                    previousStatus != PicooNative.STATUS_DISCONNECTED
                ) {
                    waitOutcome = WaitOutcome.Rejected
                }
                if (phonePairingConfirmed &&
                    senderStatus == PicooNative.STATUS_STREAMING &&
                    previousStatus != PicooNative.STATUS_STREAMING
                ) {
                    reloadTrustedStore()
                }
                if (senderStatus == PicooNative.STATUS_DISCONNECTED &&
                    previousStatus != PicooNative.STATUS_DISCONNECTED &&
                    !suppressAutoConnect
                ) {
                    runtime.clearAutoConnectAttempts()
                }
                previousStatus = senderStatus
                if (!encoderReconfiguration.isPending &&
                    streamConfigDirty.get() &&
                    (senderStatus == PicooNative.STATUS_STREAMING ||
                        senderStatus == PicooNative.STATUS_NEGOTIATING)
                ) {
                    applyStreamConfigToSender()
                }
            }
            delay(500)
        }
    }

    LaunchedEffect(senderStatus, pairingCode, phonePairingConfirmed, senderTab) {
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
                senderTab != SenderTab.ManualConnect ->
                senderTab = SenderTab.Pairing
            waitOutcome != WaitOutcome.Pending &&
                senderTab == SenderTab.Wait -> Unit
        }
    }

    DisposableEffect(runtime, encoder) {
        onDispose {
            encoder.close()
            (context as? MainActivity)?.bindSenderHandle(0L)
            runtime.close()
        }
    }

    val bitrateMbps = "%.1f Mbps".format(adaptiveBitrateBps / 1_000_000.0)
    val pairingDisplayName = selectedReceiverName.ifBlank { connectedReceiverName }

    Box(modifier = Modifier.fillMaxSize()) {
        when (senderTab) {
            SenderTab.Devices -> DevicesScreen(
                discoveredList = discoveredList,
                pairedDevices = pairedDevices,
                pairedReceiverIds = pairedReceiverIds,
                nearbyWifiGranted = nearbyWifiGranted,
                discoveryComplete = discoveryComplete,
                wifiPillText = wifiPillText,
                errorText = errorText,
                onSelectReceiver = { receiver ->
                    selectedReceiverName = receiver.displayName
                    hostText = receiver.host
                    portText = receiver.quicPort.toString()
                    selectedReceiverId = receiver.receiverId
                    connectToReceiver(receiver.host, receiver.quicPort, receiver.receiverId)
                },
                onManualConnect = { senderTab = SenderTab.ManualConnect },
                onCheckPermissions = {
                    onRequestNearbyWifi()
                    onRequestNotifications()
                },
                onRemovePaired = { device ->
                    selectedReceiverId = device.deviceId
                    val rc = runtime.removeTrustedDevice(device.deviceId)
                    if (rc == 1) {
                        runtime.forgetAutoConnectAttempt(device.deviceId)
                        if (reloadTrustedStore()) errorText = null
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
            )
            SenderTab.Settings -> SettingsScreen(
                pairedDeviceCount = pairedDevices.size,
                pairedDevices = pairedDevices,
                errorText = errorText,
                cameraGranted = cameraGranted,
                nearbyWifiGranted = nearbyWifiGranted,
                notificationsGranted = notificationsGranted,
                autoConnectEnabled = !suppressAutoConnect,
                defaultResolutionLabel = preferredResolutionLabel,
                onBack = { senderTab = SenderTab.Devices },
                onCheckPermissions = {
                    onRequestNearbyWifi()
                    onRequestNotifications()
                    onRequestCamera(null)
                },
                onOpenPairedDevices = { senderTab = SenderTab.Devices },
                onRemovePaired = { device ->
                    val rc = runtime.removeTrustedDevice(device.deviceId)
                    if (rc == 1) {
                        runtime.forgetAutoConnectAttempt(device.deviceId)
                        if (reloadTrustedStore()) errorText = null
                    } else {
                        errorText = "删除失败 ($rc)"
                    }
                },
                onToggleAutoConnect = { suppressAutoConnect = !suppressAutoConnect },
                onSelectDefaultResolution = { label ->
                    preferredResolutionLabel = StreamResolution.fromLabel(label).label
                },
            )
            SenderTab.ManualConnect -> ManualConnectScreen(
                errorText = errorText,
                onBack = { resetToDevices() },
                onConnect = { host, port ->
                    hostText = host
                    portText = port.toString()
                    selectedReceiverId = "manual-$host"
                    selectedReceiverName = host
                    connectToReceiver(host, port, selectedReceiverId)
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
                onRegenerate = { regeneratePairing() },
                onCancel = {
                    PicooNative.disconnect(senderHandle)
                    pairingCode = ""
                    connectedReceiverId = ""
                    connectedReceiverName = ""
                    phonePairingConfirmed = false
                    pairingExpired = false
                    suppressAutoConnect = true
                    resetToDevices()
                },
            )
            SenderTab.Wait -> WaitScreen(
                receiverName = pairingDisplayName,
                outcome = waitOutcome,
                onCancel = {
                    waitUserCancelled = true
                    PicooNative.disconnect(senderHandle)
                    phonePairingConfirmed = false
                    suppressAutoConnect = true
                    resetToDevices()
                },
                onBackToDevices = {
                    waitUserCancelled = true
                    PicooNative.disconnect(senderHandle)
                    phonePairingConfirmed = false
                    suppressAutoConnect = true
                    resetToDevices()
                },
                onRegenerate = { regeneratePairing() },
            )
            SenderTab.Streaming -> StreamingScreen(
                // Camera2 owns the selected buffer geometry; Compose only transforms it.
                cameraGranted = cameraGranted,
                receiverName = pairingDisplayName,
                linkQualityChip = linkQualityChip,
                resolutionLabel = resolutionLabel,
                bitrateMbps = bitrateMbps,
                lensFacing = encoder.profile.lensFacing,
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
                onRequestCamera = { onRequestCamera(null) },
                onFlipCamera = {
                    if (beginLocalEncoderReconfiguration(encoder.profile.resolution.height)) {
                        encoder.switchCamera()
                        previewTransformInfo = encoder.previewTransformInfo
                        localPreviewMirrored =
                            LocalPreviewMirror.defaultFor(encoder.profile.lensFacing)
                        encoderState = encoder.state
                        streamConfigDirty.set(true)
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
                    if (beginLocalEncoderReconfiguration(next.height)) {
                        resolutionLabel = next.label
                        if (!thermalForced720) {
                            preferredResolutionLabel = next.label
                        }
                        PicooNative.setPreferredHeight(senderHandle, next.height)
                        encoder.setTargetBitrateBps(bitrate)
                        encoder.setResolution(next.width, next.height)
                        previewTransformInfo = encoder.previewTransformInfo
                        encoderState = encoder.state
                        streamConfigDirty.set(true)
                        errorText = null
                    }
                },
                onToggleMirror = { localPreviewMirrored = !localPreviewMirrored },
                onEvMinus = {
                    val range = encoder.exposureCompensationRange
                    if (range.isEmpty()) return@StreamingScreen
                    encoder.setExposureCompensation((exposureEv - 1).coerceAtLeast(range.first.coerceAtLeast(-2)))
                    exposureEv = encoder.exposureCompensation
                },
                onEvPlus = {
                    val range = encoder.exposureCompensationRange
                    if (range.isEmpty()) return@StreamingScreen
                    encoder.setExposureCompensation((exposureEv + 1).coerceAtMost(range.last.coerceAtMost(2)))
                    exposureEv = encoder.exposureCompensation
                },
                onEvReset = {
                    if (encoder.exposureCompensationRange.isEmpty()) return@StreamingScreen
                    encoder.setExposureCompensation(0)
                    exposureEv = encoder.exposureCompensation
                },
                exposureEv = exposureEv,
                evSupported = !encoder.exposureCompensationRange.isEmpty(),
                onDisconnect = {
                    suppressAutoConnect = true
                    PicooNative.disconnect(senderHandle)
                    senderStatus = PicooNative.readSenderSnapshot(senderHandle).status
                    resetToDevices()
                },
                onStopReconnect = {
                    suppressAutoConnect = true
                    PicooNative.disconnect(senderHandle)
                    senderStatus = PicooNative.readSenderSnapshot(senderHandle).status
                    resetToDevices()
                },
                onPreviewSurfaceAvailable = { surface ->
                    encoder.bindPreviewSurface(surface)
                    previewTransformInfo = encoder.previewTransformInfo
                    encoderState = encoder.state
                    errorText = encoder.lastError
                },
                onPreviewSurfaceDestroyed = { surfaceTexture ->
                    encoder.unbindPreviewSurface(surfaceTexture)
                },
                onPreviewDisplayChanged = {
                    previewTransformInfo = encoder.refreshPreviewTransformInfo()
                },
            )
        }
    }
}
