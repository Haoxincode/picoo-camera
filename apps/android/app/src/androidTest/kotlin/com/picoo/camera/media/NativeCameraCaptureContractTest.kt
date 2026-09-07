package com.picoo.camera.media

import android.util.Log
import android.util.Size
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.runner.lifecycle.ActivityLifecycleCallback
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import java.util.concurrent.atomic.AtomicReference
import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/** REQ-PICOO-MEDIA-050: real Camera2/compositor/encoder timestamps, no image persistence. */
class NativeCameraCaptureContractTest {
    @Test
    fun backCameraDeliversRequestedNativeFormats() {
        assertTrue("Rust JNI must load", com.picoo.camera.jni.PicooNative.ensureLoaded())
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val activity = AtomicReference<MediaProbeActivity>()
        val resumed = CountDownLatch(1)
        val monitor = ActivityLifecycleMonitorRegistry.getInstance()
        val callback = ActivityLifecycleCallback { current, stage ->
            if (current is MediaProbeActivity && stage == Stage.RESUMED) {
                activity.set(current)
                resumed.countDown()
            }
        }
        monitor.addLifecycleCallback(callback)
        try {
            // OEM background-launch policy also applies to ActivityScenario.
            // Shell is allowed by the debug Activity's DUMP permission.
            instrumentation.uiAutomation.executeShellCommand(
                "am start -n ${instrumentation.targetContext.packageName}/com.picoo.camera.media.MediaProbeActivity",
            ).use { descriptor ->
                android.os.ParcelFileDescriptor.AutoCloseInputStream(descriptor).use { it.readBytes() }
            }
            assertTrue("diagnostic Activity did not resume", resumed.await(5, TimeUnit.SECONDS))
            for (codec in NativeVideoCodec.entries) {
                for (height in listOf(720, 1080)) {
                    for (fps in listOf(30, 60)) verify(codec, height, fps)
                }
            }
        } finally {
            monitor.removeLifecycleCallback(callback)
            instrumentation.runOnMainSync { activity.get()?.finish() }
        }
    }

    private fun verify(codec: NativeVideoCodec, height: Int, fps: Int) {
        val profile = CaptureProfile(
            resolution = Size(if (height == 720) 1280 else 1920, height),
            targetFps = fps,
            lensFacing = LensFacing.Back,
            codec = codec,
        )
        val targetCount = fps * 4
        val timestamps = ArrayList<Long>(targetCount)
        val errors = ArrayList<String>()
        val completed = CountDownLatch(1)
        val encoder = Camera2MediaEncoder(
            context = ApplicationProvider.getApplicationContext(),
            initialProfile = profile,
            initialBitrateBps = if (codec == NativeVideoCodec.Hevc) 16_000_000 else 24_000_000,
            initialStreamEpoch = 1,
            frameListener = { frame ->
                synchronized(timestamps) {
                    if (timestamps.size < targetCount) {
                        val config = frame.configuration
                        if (config.codec != codec || config.height != height || config.framesPerSecond != fps) {
                            if (errors.isEmpty()) errors += "output configuration differs from request"
                        }
                        timestamps += frame.presentationTimeUs
                        if (timestamps.size == targetCount) completed.countDown()
                    }
                }
            },
        )
        try {
            // Landscape sensor presentation avoids an unrelated portrait crop requirement.
            encoder.setDisplayRotationDegrees(90)
            encoder.startPreview()
            val delivered = completed.await(12, TimeUnit.SECONDS)
            assertTrue("$profile did not produce frames: ${encoder.lastError}", delivered)
            assertEquals("source request must not be downgraded", profile, encoder.profile)
            val measured = synchronized(timestamps) {
                assertTrue(errors.toString(), errors.isEmpty())
                // Ignore one second of startup; measure the next three seconds of unique PTS.
                timestamps.drop(fps)
            }
            assertTrue("PTS must increase", measured.zipWithNext().all { (a, b) -> b > a })
            val actualFps = (measured.size - 1) * 1_000_000.0 / (measured.last() - measured.first())
            Log.i("PicooCameraProbe", "codec=$codec; height=$height; requested_fps=$fps; measured_fps=$actualFps; capture=${encoder.captureSize}; camera=${encoder.selectedCameraId}; window=3s; thermal_steady_state=not_measured")
            assertTrue("$profile measured $actualFps fps", actualFps in fps * 0.95..fps * 1.05)
        } finally {
            encoder.close()
            encoder.cameraHandler.looper.thread.join(2_000)
            encoder.codecHandler.looper.thread.join(2_000)
        }
    }
}
