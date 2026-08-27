package com.picoo.camera

import android.Manifest
import android.content.pm.PackageManager
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.OutlinedTextField
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.text.input.KeyboardType
import kotlinx.coroutines.delay
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import com.picoo.camera.discovery.NsdReceiverBrowser
import com.picoo.camera.discovery.PairedAutoConnect
import com.picoo.camera.jni.PicooNative
import com.picoo.camera.media.Camera2MediaEncoder
import com.picoo.camera.media.CaptureState
import com.picoo.camera.media.EncodedFrameListener
import com.picoo.camera.media.LinkQuality
import com.picoo.camera.media.MediaBitrate
import com.picoo.camera.media.ParameterSetsListener
import com.picoo.camera.ui.CameraPreviewSurface
import com.picoo.camera.ui.QrCodeScanner
import com.picoo.camera.ui.SenderTab
import com.picoo.camera.ui.theme.PicooCameraTheme
import java.util.concurrent.atomic.AtomicReference
import android.view.WindowManager

class MainActivity : ComponentActivity() {
    private var cameraGranted by mutableStateOf(false)
    private var nearbyWifiGranted by mutableStateOf(true)
    private var pendingAfterCameraGrant: (() -> Unit)? = null
    private var activeSenderHandle: Long = 0L

    private val permissionLauncher =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            cameraGranted = granted
            val pending = pendingAfterCameraGrant
            pendingAfterCameraGrant = null
            if (granted) {
                if (activeSenderHandle != 0L) {
                    PicooNative.clearPermissionRequired(activeSenderHandle)
                }
                pending?.invoke()
            } else if (activeSenderHandle != 0L) {
                // REQ-PICOO-SESSION-001: Permission Required when CAMERA denied.
                PicooNative.markPermissionRequired(activeSenderHandle)
            }
        }

    private val nearbyWifiLauncher =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            nearbyWifiGranted = granted
        }

    /** REQ-PICOO-UI-006: request CAMERA only when an action needs it. */
    fun requestCameraPermission(then: (() -> Unit)? = null) {
        if (ContextCompat.checkSelfPermission(this, Manifest.permission.CAMERA) ==
            PackageManager.PERMISSION_GRANTED
        ) {
            cameraGranted = true
            if (activeSenderHandle != 0L) {
                PicooNative.clearPermissionRequired(activeSenderHandle)
            }
            then?.invoke()
            return
        }
        pendingAfterCameraGrant = then
        permissionLauncher.launch(Manifest.permission.CAMERA)
    }

    /** PUC-001 / DISCOVERY-005: request Nearby Wi-Fi Devices on API 33+ before NSD. */
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

    fun bindSenderHandle(handle: Long) {
        activeSenderHandle = handle
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()

        // Observe current grant only — do not prompt on cold start (REQ-PICOO-UI-006).
        cameraGranted =
            ContextCompat.checkSelfPermission(this, Manifest.permission.CAMERA) ==
                PackageManager.PERMISSION_GRANTED
        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.TIRAMISU) {
            nearbyWifiGranted =
                ContextCompat.checkSelfPermission(this, Manifest.permission.NEARBY_WIFI_DEVICES) ==
                    PackageManager.PERMISSION_GRANTED
        }

        val protocolVersion = runCatching { PicooNative.getProtocolVersion() }
            .getOrElse { "FFI unavailable: ${it.message}" }

        setContent {
            PicooCameraTheme {
                Scaffold(modifier = Modifier.fillMaxSize()) { innerPadding ->
                    SenderHomeScreen(
                        protocolVersion = protocolVersion,
                        cameraGranted = cameraGranted,
                        nearbyWifiGranted = nearbyWifiGranted,
                        onRequestCamera = { then -> requestCameraPermission(then) },
                        onRequestNearbyWifi = { ensureNearbyWifiPermission() },
                        modifier = Modifier.padding(innerPadding),
                    )
                }
            }
        }
    }
}

@Composable
private fun SenderHomeScreen(
    protocolVersion: String,
    cameraGranted: Boolean,
    nearbyWifiGranted: Boolean,
    onRequestCamera: (then: (() -> Unit)?) -> Unit,
    onRequestNearbyWifi: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val context = androidx.compose.ui.platform.LocalContext.current
    var encodedFrames by remember { mutableIntStateOf(0) }
    var keyFrames by remember { mutableIntStateOf(0) }
    var rustPackets by remember { mutableIntStateOf(0) }
    var encoderState by remember { mutableStateOf(CaptureState.Idle) }
    var statsText by remember { mutableStateOf("") }
    var errorText by remember { mutableStateOf<String?>(null) }
    var hostText by remember { mutableStateOf("127.0.0.1") }
    var portText by remember { mutableStateOf("4433") }
    var senderStatus by remember { mutableIntStateOf(PicooNative.STATUS_DISCONNECTED) }
    var pairingCode by remember { mutableStateOf("") }
    val discoveredListState =
        remember { mutableStateOf<List<PicooNative.DiscoveredReceiver>>(emptyList()) }
    val discoveredList = discoveredListState.value
    var connectedReceiverId by remember { mutableStateOf("") }
    var pairedDevicesText by remember { mutableStateOf("") }
    var selectedReceiverId by remember { mutableStateOf("") }
    var trustedStoreHandle by remember { mutableLongStateOf(0L) }
    val pairedReceiverIds = remember { mutableStateOf<Set<String>>(emptySet()) }
    val autoConnectAttemptedIds = remember { mutableStateOf<Set<String>>(emptySet()) }
    var suppressAutoConnect by remember { mutableStateOf(false) }
    var qrJsonText by remember { mutableStateOf("") }
    var showQrScanner by remember { mutableStateOf(false) }
    var remoteMirrored by remember { mutableStateOf(false) }
    var localPreviewMirrored by remember { mutableStateOf(true) }
    var resolutionLabel by remember { mutableStateOf("720p") }
    var powerHint by remember { mutableStateOf("") }
    var linkStatsText by remember { mutableStateOf("") }
    var adaptiveBitrateBps by remember { mutableIntStateOf(3_000_000) }
    var exposureEv by remember { mutableIntStateOf(0) }
    var senderTab by remember { mutableStateOf(SenderTab.Devices) }
    val parameterSetsRef = remember { AtomicReference<Pair<ByteArray, ByteArray>?>(null) }
    val streamConfigDirty = remember { java.util.concurrent.atomic.AtomicBoolean(false) }

    var diagnosticExportPath by remember { mutableStateOf("") }

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
            }
        }

    LaunchedEffect(nearbyWifiGranted) {
        if (!nearbyWifiGranted) {
            onRequestNearbyWifi()
        }
    }

    DisposableEffect(nsdBrowser, nearbyWifiGranted) {
        // REQ-PICOO-DISCOVERY-005 / ARCH-PICOO-DISCOVERY-001: Android uses NSD, not Rust mDNS.
        if (nearbyWifiGranted) {
            nsdBrowser.start()
        }
        onDispose { nsdBrowser.stop() }
    }

    val encoder = remember {
        Camera2MediaEncoder(
            context = context,
            frameListener = EncodedFrameListener { data, isKeyFrame, ptsUs, streamEpoch ->
                encodedFrames += 1
                if (isKeyFrame) keyFrames += 1
                val fragments = PicooNative.ingestAccessUnit(
                    handle = senderHandle,
                    data = data,
                    keyframe = isKeyFrame,
                    ptsUs = ptsUs,
                    streamEpoch = streamEpoch,
                )
                if (fragments > 0) {
                    rustPackets += fragments
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
            sps = sets?.first,
            pps = sets?.second,
        )
        streamConfigDirty.set(false)
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
            errorText = "Connect failed: $rc"
        }
    }

    fun connectFromQrJson(json: String) {
        val payload = PicooNative.parseQrConnect(json.trim())
        if (payload == null) {
            errorText = "Invalid or expired QR payload"
            return
        }
        hostText = payload.host
        portText = payload.quicPort.toString()
        selectedReceiverId = payload.receiverId
        connectToReceiver(payload.host, payload.quicPort, payload.receiverId)
    }

    fun reloadTrustedStore() {
        if (trustedStoreHandle != 0L) {
            PicooNative.destroyTrustedStore(trustedStoreHandle)
        }
        trustedStoreHandle = PicooNative.loadTrustedStore(trustedStorePath)
        val ids = linkedSetOf<String>()
        pairedDevicesText = buildString {
            if (trustedStoreHandle == 0L) return@buildString
            val count = PicooNative.getTrustedDeviceCount(trustedStoreHandle)
            for (index in 0 until count) {
                val device = PicooNative.getTrustedDevice(trustedStoreHandle, index) ?: continue
                ids.add(device.deviceId)
                append("${device.deviceName} (${device.deviceId})\n")
            }
        }.trim()
        pairedReceiverIds.value = ids
    }

    LaunchedEffect(senderHandle, trustedStorePath) {
        if (senderHandle != 0L) {
            PicooNative.attachTrustedStore(senderHandle, trustedStorePath)
        }
        reloadTrustedStore()
    }

    // PUC-002: once a paired receiver appears in NSD, connect without requiring a tap.
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
    val view = LocalView.current
    androidx.compose.runtime.SideEffect {
        // REQ-PICOO-UI-005: prevent lock screen while actively streaming.
        val window = (view.context as? android.app.Activity)?.window
        if (keepScreenOn) {
            window?.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        } else {
            window?.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        }
    }

    LaunchedEffect(senderStatus) {
        when (senderStatus) {
            PicooNative.STATUS_STREAMING,
            PicooNative.STATUS_NEGOTIATING,
            PicooNative.STATUS_PAIRING,
            PicooNative.STATUS_CONNECTING,
            PicooNative.STATUS_NETWORK_UNSTABLE,
            PicooNative.STATUS_RECONNECTING,
            -> StreamingForegroundService.start(context)
            else -> StreamingForegroundService.stop(context)
        }
    }

    // Continuous battery + thermal polling while the session is live (REQ-PICOO-UI-005).
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
            return@LaunchedEffect
        }
        while (true) {
            powerHint = PowerHints.readHint(context)
            delay(5_000)
        }
    }

    LaunchedEffect(senderHandle) {
        var previousStatus = PicooNative.STATUS_DISCONNECTED
        while (true) {
            if (senderHandle != 0L) {
                PicooNative.pump(senderHandle)
                senderStatus = PicooNative.getSenderStatus(senderHandle)
                pairingCode = PicooNative.getPairingShortCode(senderHandle)
                connectedReceiverId = PicooNative.getConnectedReceiverId(senderHandle)
                val bps = PicooNative.getCurrentBitrateBps(senderHandle)
                if (bps > 0) {
                    adaptiveBitrateBps = bps
                    encoder.setTargetBitrateBps(bps)
                }
                val link = PicooNative.getLinkStats(senderHandle)
                linkStatsText = if (link != null && link.size >= 6) {
                    LinkQuality.formatLine(
                        rttMs = link[0],
                        packetLoss = link[1],
                        frameAgeMs = link[3],
                        receiveBitrate = link[4],
                    )
                } else {
                    ""
                }
                if (PicooNative.takeKeyframeRequest(senderHandle) == 1) {
                    encoder.requestKeyFrame()
                }
                if (previousStatus == PicooNative.STATUS_RECONNECTING &&
                    senderStatus == PicooNative.STATUS_STREAMING
                ) {
                    // REQ-PICOO-SESSION-004: recover with IDR + refreshed StreamConfig.
                    encoder.requestKeyFrame()
                    streamConfigDirty.set(true)
                }
                // Allow NSD auto-connect to retry after unexpected disconnect,
                // but not after an intentional user stop (PUC-005 Disconnect).
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

    LaunchedEffect(senderStatus, pairingCode) {
        senderTab = when {
            senderStatus == PicooNative.STATUS_STREAMING ||
                senderStatus == PicooNative.STATUS_NEGOTIATING ||
                senderStatus == PicooNative.STATUS_RECONNECTING ||
                senderStatus == PicooNative.STATUS_NETWORK_UNSTABLE -> SenderTab.Streaming
            senderStatus == PicooNative.STATUS_PERMISSION_REQUIRED -> SenderTab.Streaming
            pairingCode.isNotEmpty() || senderStatus == PicooNative.STATUS_PAIRING -> SenderTab.Pairing
            else -> senderTab
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

    PicooCameraTheme(darkTheme = senderTab == SenderTab.Streaming) {
    Box(
        modifier = modifier.fillMaxSize(),
    ) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(
            text = "Picoo Camera Sender",
            style = MaterialTheme.typography.headlineSmall,
        )
        Text(text = "Protocol: $protocolVersion", style = MaterialTheme.typography.bodyMedium)
        Text(
            text = "Session: ${PicooNative.statusLabel(senderStatus)}",
            style = MaterialTheme.typography.bodyMedium,
        )
        // REQ-PICOO-UI-003 — Devices / Pairing / Streaming pages (PRD §17).
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            listOf(
                SenderTab.Devices to "Devices",
                SenderTab.Pairing to "Pairing",
                SenderTab.Streaming to "Streaming",
            ).forEach { (tab, label) ->
                Button(onClick = { senderTab = tab }) {
                    Text(if (senderTab == tab) "[$label]" else label)
                }
            }
        }

        when (senderTab) {
            SenderTab.Devices -> {
                if (!nearbyWifiGranted) {
                    Text(
                        text = "Nearby Wi-Fi permission required for discovery on Android 13+",
                        color = MaterialTheme.colorScheme.error,
                        style = MaterialTheme.typography.bodySmall,
                    )
                    Button(onClick = onRequestNearbyWifi) {
                        Text("Grant nearby Wi-Fi")
                    }
                }
                if (discoveredList.isNotEmpty()) {
                    Text(text = "Discovered (NSD):", style = MaterialTheme.typography.bodySmall)
                    discoveredList.forEach { receiver ->
                        Button(
                            onClick = {
                                hostText = receiver.host
                                portText = receiver.quicPort.toString()
                                selectedReceiverId = receiver.receiverId
                                connectToReceiver(receiver.host, receiver.quicPort, receiver.receiverId)
                            },
                            modifier = Modifier.fillMaxWidth(),
                        ) {
                            Text("${receiver.displayName} (${receiver.host}:${receiver.quicPort})")
                        }
                    }
                } else {
                    Text(
                        text = "Searching for receivers via NSD (_picoocam._udp)…",
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
                if (pairedDevicesText.isNotEmpty()) {
                    Text(text = "Paired receivers:\n$pairedDevicesText", style = MaterialTheme.typography.bodySmall)
                }
                OutlinedTextField(
                    value = hostText,
                    onValueChange = { hostText = it },
                    label = { Text("Receiver host") },
                    modifier = Modifier.fillMaxWidth(),
                )
                OutlinedTextField(
                    value = portText,
                    onValueChange = { portText = it },
                    label = { Text("QUIC port") },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    modifier = Modifier.fillMaxWidth(),
                )
                OutlinedTextField(
                    value = qrJsonText,
                    onValueChange = { qrJsonText = it },
                    label = { Text("QR JSON (paste or scan)") },
                    modifier = Modifier.fillMaxWidth(),
                    minLines = 2,
                )
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Button(onClick = {
                        val port = portText.toIntOrNull() ?: return@Button
                        connectToReceiver(hostText, port, selectedReceiverId.ifEmpty { "windows-receiver" })
                    }) {
                        Text("Connect")
                    }
                    Button(onClick = {
                        if (qrJsonText.isBlank()) {
                            errorText = "Paste QR JSON or scan first"
                            return@Button
                        }
                        connectFromQrJson(qrJsonText)
                    }) {
                        Text("Connect from QR")
                    }
                    Button(onClick = {
                        onRequestCamera { showQrScanner = true }
                    }) {
                        Text("Scan QR")
                    }
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Button(onClick = {
                        val deviceId = selectedReceiverId.ifEmpty {
                            pairedDevicesText.lines().firstOrNull()?.substringAfter("(")?.substringBefore(")") ?: ""
                        }
                        if (deviceId.isEmpty()) {
                            errorText = "Select a paired device to remove"
                            return@Button
                        }
                        val rc = PicooNative.removeTrustedDevice(trustedStoreHandle, deviceId)
                        if (rc == 1) {
                            PicooNative.saveTrustedStore(trustedStoreHandle)
                            PicooNative.attachTrustedStore(senderHandle, trustedStorePath)
                            reloadTrustedStore()
                            errorText = null
                        } else {
                            errorText = "Remove failed: $rc"
                        }
                    }) {
                        Text("Remove paired")
                    }
                    Button(onClick = {
                        val outFile = java.io.File(context.cacheDir, "picoo-diagnostics.json")
                        val rc = PicooNative.exportDiagnosticsToPath(
                            trustedStorePath,
                            platform = "android",
                            appVersion = "0.1.0",
                            outPath = outFile.absolutePath,
                        )
                        diagnosticExportPath = if (rc == 0) {
                            errorText = null
                            outFile.absolutePath
                        } else {
                            errorText = "Diagnostics export failed: $rc"
                            ""
                        }
                    }) {
                        Text("Export diagnostics")
                    }
                }
                if (diagnosticExportPath.isNotEmpty()) {
                    Text(
                        text = "Diagnostics: $diagnosticExportPath (redacted, no video)",
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
                errorText?.let {
                    Text(text = "Error: $it", color = MaterialTheme.colorScheme.error)
                }
            }
            SenderTab.Pairing -> {
                Text(
                    text = if (pairingCode.isNotEmpty()) "Confirm Pairing" else "Waiting for pairing…",
                    style = MaterialTheme.typography.titleMedium,
                )
                if (connectedReceiverId.isNotEmpty()) {
                    Text(text = "Receiver: $connectedReceiverId")
                }
                if (pairingCode.isNotEmpty()) {
                    Text(text = pairingCode, style = MaterialTheme.typography.headlineMedium)
                    Text(text = "Make sure the same number appears on your computer.")
                }
                Button(onClick = {
                    val receiverId = connectedReceiverId.ifEmpty { selectedReceiverId.ifEmpty { "windows-receiver" } }
                    val rc = PicooNative.sendPairingConfirm(senderHandle, receiverId)
                    if (rc == 0) {
                        errorText = null
                        reloadTrustedStore()
                        senderTab = SenderTab.Streaming
                    } else {
                        errorText = "Pairing confirm failed: $rc"
                    }
                }) {
                    Text("Confirm pairing")
                }
                errorText?.let {
                    Text(text = "Error: $it", color = MaterialTheme.colorScheme.error)
                }
            }
            SenderTab.Streaming -> {
                if (powerHint.isNotEmpty()) {
                    Text(
                        text = powerHint,
                        color = MaterialTheme.colorScheme.error,
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
                if (linkStatsText.isNotEmpty()) {
                    Text(
                        text = linkStatsText,
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text("Remote mirror (sent to PC)")
                    Switch(checked = remoteMirrored, onCheckedChange = {
                        remoteMirrored = it
                        applyStreamConfigToSender()
                    })
                }
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text("Local preview mirror")
                    Switch(
                        checked = localPreviewMirrored,
                        onCheckedChange = { localPreviewMirrored = it },
                    )
                }
                if (cameraGranted) {
                    CameraPreviewSurface(
                        modifier = Modifier
                            .fillMaxWidth()
                            .height(240.dp),
                        mirrorLocal = localPreviewMirrored,
                        onSurfaceAvailable = { surface ->
                            encoder.bindPreviewSurface(surface)
                            encoderState = encoder.state
                            errorText = encoder.lastError
                        },
                        onSurfaceDestroyed = { encoder.unbindPreviewSurface() },
                    )
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        Button(onClick = {
                            encoder.switchCamera()
                            encoderState = encoder.state
                            applyStreamConfigToSender()
                        }) {
                            Text("Switch camera")
                        }
                        Button(onClick = {
                            val maxH = PicooNative.getReceiverMaxHeight(senderHandle)
                            val want1080 = resolutionLabel == "720p"
                            if (want1080 && maxH in 1 until 1080) {
                                errorText = "Receiver caps at ${maxH}p — staying 720p"
                                return@Button
                            }
                            resolutionLabel = if (want1080) "1080p" else "720p"
                            val (w, h) = if (resolutionLabel == "1080p") 1920 to 1080 else 1280 to 720
                            encoder.setResolution(w, h)
                            encoderState = encoder.state
                            applyStreamConfigToSender()
                        }) {
                            Text("Resolution: $resolutionLabel")
                        }
                        Button(onClick = {
                            val rust = PicooNative.readSenderStats(senderHandle)
                            statsText = buildString {
                                append("enc=$encodedFrames keys=$keyFrames ")
                                append("~${encoder.stats.lastBitrateEstimateKbps}kbps ")
                                append("epoch=${encoder.streamEpoch}\n")
                                append("rust AU=${rust.accessUnits} pkts=${rust.packets} ")
                                append("sent=${rust.sentDatagrams} pending=${rust.pendingPackets}")
                            }
                            encoderState = encoder.state
                            errorText = encoder.lastError
                        }) {
                            Text("Refresh stats")
                        }
                        Button(onClick = {
                            // PUC-005: intentional stop — stay disconnected, no NSD auto-rejoin.
                            suppressAutoConnect = true
                            PicooNative.disconnect(senderHandle)
                            senderStatus = PicooNative.getSenderStatus(senderHandle)
                            senderTab = SenderTab.Devices
                            errorText = null
                        }) {
                            Text("Disconnect")
                        }
                    }
                    val evRange = encoder.exposureCompensationRange
                    if (!evRange.isEmpty()) {
                        Row(
                            modifier = Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.spacedBy(8.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text("EV $exposureEv", style = MaterialTheme.typography.bodySmall)
                            Button(onClick = {
                                encoder.setExposureCompensation(exposureEv - 1)
                                exposureEv = encoder.exposureCompensation
                            }) {
                                Text("EV -")
                            }
                            Button(onClick = {
                                encoder.setExposureCompensation(exposureEv + 1)
                                exposureEv = encoder.exposureCompensation
                            }) {
                                Text("EV +")
                            }
                        }
                    }
                    Text(text = "State: $encoderState", style = MaterialTheme.typography.bodySmall)
                    Text(text = statsText, style = MaterialTheme.typography.bodySmall)
                    errorText?.let {
                        Text(text = "Error: $it", color = MaterialTheme.colorScheme.error)
                    }
                } else {
                    Spacer(modifier = Modifier.height(8.dp))
                    Text(
                        text = "Camera access is requested only when you enable preview or scan QR.",
                        style = MaterialTheme.typography.bodySmall,
                    )
                    Button(onClick = { onRequestCamera(null) }) {
                        Text("Enable camera")
                    }
                }
            }
        }
    }

    if (showQrScanner && cameraGranted) {
        QrCodeScanner(
            modifier = Modifier.fillMaxSize(),
            onQrDetected = { payload ->
                qrJsonText = payload
                connectFromQrJson(payload)
                showQrScanner = false
            },
            onClose = { showQrScanner = false },
        )
    }
    } // Box
    } // PicooCameraTheme(darkTheme=Streaming)
}
