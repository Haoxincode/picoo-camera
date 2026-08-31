package com.picoo.camera

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Bundle
import android.provider.Settings
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.lifecycle.viewmodel.compose.viewModel
import com.picoo.camera.jni.PicooNative
import com.picoo.camera.ui.SenderHomeScreen
import com.picoo.camera.ui.theme.PicooCameraTheme

class MainActivity : ComponentActivity() {
    private var cameraGranted by mutableStateOf(false)
    private var cameraPermissionPermanentlyDenied by mutableStateOf(false)
    private var nearbyWifiGranted by mutableStateOf(true)
    private var notificationsGranted by mutableStateOf(true)
    private var cameraPermissionRequestInFlight = false
    private var cameraPermissionAutoRequested = false
    private var activeSenderHandle: Long = 0L
    private val permissionPreferences by lazy {
        getSharedPreferences("picoo_permissions", MODE_PRIVATE)
    }

    private val permissionLauncher =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            cameraPermissionRequestInFlight = false
            cameraGranted = granted
            cameraPermissionPermanentlyDenied =
                !granted && !shouldShowRequestPermissionRationale(Manifest.permission.CAMERA)
            if (granted) {
                if (activeSenderHandle != 0L && PicooNative.isAvailable) {
                    PicooNative.clearPermissionRequired(activeSenderHandle)
                }
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

    fun requestCameraPermission(userInitiated: Boolean) {
        if (ContextCompat.checkSelfPermission(this, Manifest.permission.CAMERA) ==
            PackageManager.PERMISSION_GRANTED
        ) {
            cameraGranted = true
            cameraPermissionPermanentlyDenied = false
            if (activeSenderHandle != 0L && PicooNative.isAvailable) {
                PicooNative.clearPermissionRequired(activeSenderHandle)
            }
            return
        }
        if (cameraPermissionRequestInFlight) return
        if (!userInitiated && cameraPermissionAutoRequested) return
        if (cameraPermissionPermanentlyDenied) {
            if (userInitiated) {
                startActivity(
                    Intent(
                        Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
                        Uri.fromParts("package", packageName, null),
                    ),
                )
            }
            return
        }
        cameraPermissionAutoRequested = true
        cameraPermissionRequestInFlight = true
        permissionPreferences.edit().putBoolean("camera_requested", true).apply()
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
        cameraPermissionPermanentlyDenied =
            !cameraGranted &&
            permissionPreferences.getBoolean("camera_requested", false) &&
            !shouldShowRequestPermissionRationale(Manifest.permission.CAMERA)
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
                        sessionModel = viewModel(),
                        cameraGranted = cameraGranted,
                        cameraPermissionPermanentlyDenied = cameraPermissionPermanentlyDenied,
                        nearbyWifiGranted = nearbyWifiGranted,
                        notificationsGranted = notificationsGranted,
                        onEnsureCamera = { requestCameraPermission(userInitiated = false) },
                        onRequestCamera = { requestCameraPermission(userInitiated = true) },
                        onRequestNearbyWifi = { ensureNearbyWifiPermission() },
                        onRequestNotifications = { ensureNotificationsPermission() },
                    )
                }
            }
        }
    }

    override fun onResume() {
        super.onResume()
        val granted = ContextCompat.checkSelfPermission(this, Manifest.permission.CAMERA) ==
            PackageManager.PERMISSION_GRANTED
        cameraGranted = granted
        if (granted) {
            cameraPermissionPermanentlyDenied = false
            cameraPermissionAutoRequested = false
            if (activeSenderHandle != 0L && PicooNative.isAvailable) {
                PicooNative.clearPermissionRequired(activeSenderHandle)
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
