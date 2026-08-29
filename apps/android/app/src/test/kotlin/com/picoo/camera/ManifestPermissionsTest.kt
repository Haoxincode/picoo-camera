package com.picoo.camera

import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/**
 * REQ-PICOO-UI-005 / UI-006 / PUC-001: critical permissions must stay declared.
 * JVM-readable from source AndroidManifest (no device).
 */
class ManifestPermissionsTest {
    @Test
    fun declaresCameraNetworkNearbyAndNotifications() {
        val manifest = locateManifest()
        val text = manifest.readText()
        val required =
            listOf(
                "android.permission.CAMERA",
                "android.permission.INTERNET",
                "android.permission.ACCESS_NETWORK_STATE",
                "android.permission.CHANGE_WIFI_MULTICAST_STATE",
                "android.permission.NEARBY_WIFI_DEVICES",
                "android.permission.FOREGROUND_SERVICE",
                "android.permission.FOREGROUND_SERVICE_CAMERA",
                "android.permission.POST_NOTIFICATIONS",
                "android.permission.WAKE_LOCK",
            )
        for (perm in required) {
            assertTrue(
                "missing <uses-permission> for $perm in ${manifest.path}",
                text.contains("android:name=\"$perm\""),
            )
        }
    }

    private fun locateManifest(): File {
        val candidates =
            listOf(
                File("src/main/AndroidManifest.xml"),
                File("app/src/main/AndroidManifest.xml"),
                File("apps/android/app/src/main/AndroidManifest.xml"),
            )
        return candidates.firstOrNull { it.isFile }
            ?: error("AndroidManifest.xml not found from ${File(".").absolutePath}")
    }
}
