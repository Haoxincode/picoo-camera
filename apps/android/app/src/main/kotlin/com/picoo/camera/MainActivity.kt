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
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import com.picoo.camera.jni.PicooNative
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
    Column(
        modifier = modifier
            .fillMaxSize()
            .padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp, Alignment.CenterVertically),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(
            text = "Picoo Camera Sender",
            style = MaterialTheme.typography.headlineMedium,
        )
        Text(
            text = "Protocol: $protocolVersion",
            style = MaterialTheme.typography.bodyLarge,
        )
        Text(
            text = if (cameraGranted) {
                "Camera permission granted — discovery UI next"
            } else {
                "Camera permission required for capture"
            },
            style = MaterialTheme.typography.bodyMedium,
        )
    }
}
