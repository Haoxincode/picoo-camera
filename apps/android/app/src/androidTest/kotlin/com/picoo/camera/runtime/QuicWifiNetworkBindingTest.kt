package com.picoo.camera.runtime

import androidx.test.platform.app.InstrumentationRegistry
import com.picoo.camera.jni.PicooNative
import org.junit.Assert.assertEquals
import org.junit.Assume.assumeTrue
import org.junit.Test

class QuicWifiNetworkBindingTest {
    @Test
    fun quicClientSocketBindsToPhysicalWifi() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext.applicationContext
        val runtime = SenderNativeRuntime(context)
        try {
            val deadlineNanos = System.nanoTime() + WIFI_CALLBACK_TIMEOUT_NANOS
            var binding = runtime.configureQuicWifiNetwork(TEST_ENDPOINT_HOST)
            while (binding == QuicWifiBindingResult.WifiUnavailable &&
                System.nanoTime() < deadlineNanos
            ) {
                Thread.sleep(WIFI_CALLBACK_POLL_MS)
                binding = runtime.configureQuicWifiNetwork(TEST_ENDPOINT_HOST)
            }

            assumeTrue(
                "instrumented device has no physical Wi-Fi",
                binding != QuicWifiBindingResult.WifiUnavailable,
            )
            assertEquals(QuicWifiBindingResult.Bound, binding)

            // Socket creation and android_setsocknetwork happen synchronously inside connect;
            // the documentation-only address does not need a Receiver to be listening.
            assertEquals(0, PicooNative.connect(runtime.senderHandle, TEST_ENDPOINT_HOST, 4433))
            PicooNative.disconnect(runtime.senderHandle)
        } finally {
            runtime.close()
        }
    }

    private companion object {
        const val WIFI_CALLBACK_POLL_MS = 50L
        const val WIFI_CALLBACK_TIMEOUT_NANOS = 5_000_000_000L
        const val TEST_ENDPOINT_HOST = "192.0.2.1"
    }
}
