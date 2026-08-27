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
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
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

    val senderHandle = remember { PicooNative.createSender() }

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

    DisposableEffect(encoder, senderHandle) {
        onDispose {
            encoder.close()
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
