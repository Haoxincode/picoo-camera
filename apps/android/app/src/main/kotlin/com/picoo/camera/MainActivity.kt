package com.picoo.camera

import android.Manifest
import android.content.pm.PackageManager
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
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
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import com.picoo.camera.jni.PicooNative
import com.picoo.camera.media.Camera2MediaEncoder
import com.picoo.camera.media.CaptureState
import com.picoo.camera.media.EncodedFrameListener
import com.picoo.camera.ui.CameraPreviewSurface
import com.picoo.camera.ui.theme.PicooCameraTheme

class MainActivity : ComponentActivity() {
    private var cameraGranted by mutableStateOf(false)

    private val permissionLauncher =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            cameraGranted = granted
        }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()

        cameraGranted =
            ContextCompat.checkSelfPermission(this, Manifest.permission.CAMERA) ==
                PackageManager.PERMISSION_GRANTED
        if (!cameraGranted) {
            permissionLauncher.launch(Manifest.permission.CAMERA)
        }

        val protocolVersion = runCatching { PicooNative.getProtocolVersion() }
            .getOrElse { "FFI unavailable: ${it.message}" }

        setContent {
            PicooCameraTheme {
                Scaffold(modifier = Modifier.fillMaxSize()) { innerPadding ->
                    SenderHomeScreen(
                        protocolVersion = protocolVersion,
                        cameraGranted = cameraGranted,
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
    var discoveredReceivers by remember { mutableStateOf("") }
    var connectedReceiverId by remember { mutableStateOf("") }
    var pairedDevicesText by remember { mutableStateOf("") }
    var selectedReceiverId by remember { mutableStateOf("") }
    var trustedStoreHandle by remember { mutableLongStateOf(0L) }

    var diagnosticExportPath by remember { mutableStateOf("") }

    val trustedStorePath = remember {
        java.io.File(context.filesDir, "trusted_devices.json").absolutePath
    }
    val senderHandle = remember { PicooNative.createSender() }
    val browserHandle = remember { PicooNative.createDiscoveryBrowser() }

    fun reloadTrustedStore() {
        if (trustedStoreHandle != 0L) {
            PicooNative.destroyTrustedStore(trustedStoreHandle)
        }
        trustedStoreHandle = PicooNative.loadTrustedStore(trustedStorePath)
        pairedDevicesText = buildString {
            if (trustedStoreHandle == 0L) return@buildString
            val count = PicooNative.getTrustedDeviceCount(trustedStoreHandle)
            for (index in 0 until count) {
                val device = PicooNative.getTrustedDevice(trustedStoreHandle, index) ?: continue
                append("${device.deviceName} (${device.deviceId})\n")
            }
        }.trim()
    }

    LaunchedEffect(senderHandle, trustedStorePath) {
        if (senderHandle != 0L) {
            PicooNative.attachTrustedStore(senderHandle, trustedStorePath)
        }
        reloadTrustedStore()
    }

    LaunchedEffect(senderHandle, browserHandle) {
        while (true) {
            if (senderHandle != 0L) {
                PicooNative.pump(senderHandle)
                senderStatus = PicooNative.getSenderStatus(senderHandle)
                pairingCode = PicooNative.getPairingShortCode(senderHandle)
                connectedReceiverId = PicooNative.getConnectedReceiverId(senderHandle)
            }
            if (browserHandle != 0L) {
                PicooNative.pollDiscoveryBrowser(browserHandle, 200)
                val count = PicooNative.getDiscoveryCount(browserHandle)
                discoveredReceivers = buildString {
                    for (index in 0 until count) {
                        val receiver = PicooNative.getDiscoveredReceiver(browserHandle, index)
                        if (receiver != null) {
                            append("${receiver.displayName} (${receiver.host}:${receiver.quicPort})\n")
                        }
                    }
                }.trim()
            }
            delay(500)
        }
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
        )
    }

    DisposableEffect(encoder, senderHandle, browserHandle, trustedStoreHandle) {
        onDispose {
            encoder.close()
            if (browserHandle != 0L) {
                PicooNative.destroyDiscoveryBrowser(browserHandle)
            }
            if (trustedStoreHandle != 0L) {
                PicooNative.destroyTrustedStore(trustedStoreHandle)
            }
            if (senderHandle != 0L) {
                PicooNative.destroySender(senderHandle)
            }
        }
    }

    Column(
        modifier = modifier
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
        if (pairingCode.isNotEmpty()) {
            Text(text = "Pairing code: $pairingCode", style = MaterialTheme.typography.bodyLarge)
        }
        if (discoveredReceivers.isNotEmpty()) {
            Text(text = "Discovered:\n$discoveredReceivers", style = MaterialTheme.typography.bodySmall)
        }
        if (pairedDevicesText.isNotEmpty()) {
            Text(text = "Paired receivers:\n$pairedDevicesText", style = MaterialTheme.typography.bodySmall)
        }
        if (connectedReceiverId.isNotEmpty()) {
            Text(text = "Connected receiver: $connectedReceiverId", style = MaterialTheme.typography.bodySmall)
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
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Button(onClick = {
                val port = portText.toIntOrNull() ?: return@Button
                PicooNative.setStreamConfig(
                    senderHandle,
                    width = 1280,
                    height = 720,
                    fps = 30,
                    bitrateBps = 6_000_000,
                    streamEpoch = encoder.streamEpoch,
                    mirrored = false,
                )
                val rc = PicooNative.connect(senderHandle, hostText.trim(), port)
                if (rc == 0) {
                    PicooNative.sendClientHello(
                        senderHandle,
                        senderId = "android-sender",
                        deviceName = android.os.Build.MODEL,
                        publicKey = byteArrayOf(1, 2, 3),
                    )
                    errorText = null
                } else {
                    errorText = "Connect failed: $rc"
                }
            }) {
                Text("Connect")
            }
            Button(onClick = {
                val receiverId = connectedReceiverId.ifEmpty { selectedReceiverId.ifEmpty { "windows-receiver" } }
                val rc = PicooNative.sendPairingConfirm(senderHandle, receiverId)
                if (rc == 0) {
                    errorText = null
                    reloadTrustedStore()
                } else {
                    errorText = "Pairing confirm failed: $rc"
                }
            }) {
                Text("Confirm pairing")
            }
            Button(onClick = {
                val deviceId = selectedReceiverId.ifEmpty { pairedDevicesText.lines().firstOrNull()?.substringAfter("(")?.substringBefore(")") ?: "" }
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

        if (cameraGranted) {
            CameraPreviewSurface(
                modifier = Modifier
                    .fillMaxWidth()
                    .height(240.dp),
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
                }) {
                    Text("Switch camera")
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
            }

            Text(text = "State: $encoderState", style = MaterialTheme.typography.bodySmall)
            Text(text = statsText, style = MaterialTheme.typography.bodySmall)
            errorText?.let {
                Text(text = "Error: $it", color = MaterialTheme.colorScheme.error)
            }
        } else {
            Spacer(modifier = Modifier.height(8.dp))
            Text(
                text = "Camera permission required",
                modifier = Modifier.align(Alignment.CenterHorizontally),
            )
        }
    }
}
