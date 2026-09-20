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
    fun backCameraDeliversRequestedNativeFormats() = withProbeActivity {
        for (codec in NativeVideoCodec.entries) {
            for (height in listOf(720, 1080)) {
                for (fps in listOf(30, 60)) verify(codec, height, fps)
            }
        }
    }

    private fun withProbeActivity(block: () -> Unit) {
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
            block()
        } finally {
            monitor.removeLifecycleCallback(callback)
            instrumentation.runOnMainSync { activity.get()?.finish() }
        }
    }

    /** REQ-PICOO-MEDIA-058: orientation rebuilds input coverage and restores full facts. */
    @Test
    fun orientationRebuildsInputAndRestoresCommittedProfile() = withProbeActivity {
        val original = CaptureProfile(Size(1920, 1080), 60, LensFacing.Back,
            NativeVideoCodec.Avc, displayRotationDegrees = 90)
        val facts = CaptureFacts()
        val bitrate = com.picoo.camera.jni.PicooNative.bitrateInitialForHeight(1080)
        val encoder = Camera2MediaEncoder(
            context = ApplicationProvider.getApplicationContext(),
            initialProfile = original, initialBitrateBps = bitrate, initialStreamEpoch = 1,
            frameListener = facts,
        )
        fun sample(epoch: Int, afterGeneration: Long) = facts.sample(encoder, epoch, afterGeneration, 60)
        try {
            encoder.startPreview()
            val first = sample(1, 0)
            val originalInput = encoder.captureSize
            encoder.prepareStreamEpoch(2)
            encoder.setDisplayRotationDegrees(0)
            encoder.setSourceFormat(VideoSourceFormat.Default)
            val second = sample(2, first)
            assertEquals(0, encoder.profile.displayRotationDegrees)
            assertTrue("portrait crop must have enough native pixels: ${encoder.captureSize}",
                encoder.captureSize.height >= 1920)
            encoder.restoreCommittedConfiguration(original, 1, bitrate)
            sample(1, second)
            assertEquals(original, encoder.profile)
            assertEquals(originalInput, encoder.captureSize)
        } finally {
            encoder.close()
            encoder.cameraHandler.looper.thread.join(2_000)
            encoder.codecHandler.looper.thread.join(2_000)
        }
    }

    /** MEDIA-060: a supported portrait format really switches to front and back. */
    @Test
    fun cameraSwitchChoosesTargetCeilingAndRestores() = withProbeActivity {
        val original = CaptureProfile(Size(1920, 1080), 60, LensFacing.Back,
            NativeVideoCodec.Avc, displayRotationDegrees = 0)
        val facts = CaptureFacts()
        val bitrate = com.picoo.camera.jni.PicooNative.bitrateInitialForHeight(1080)
        val encoder = Camera2MediaEncoder(ApplicationProvider.getApplicationContext(),
            original, bitrate, 1, facts)
        try {
            val available = SourcePreparation.candidates(encoder.cameraManager, LensFacing.Front,
                0, VideoSourceFormat.ProductFormats) { bitrate }.getOrThrow()
            val target = checkNotNull(CameraSourceSelection.select(available, original.codec))
            assertEquals(VideoSourceFormat(NativeVideoCodec.Avc, StreamResolution.P1080, 30), target)
            encoder.startPreview()
            val first = facts.sample(encoder, 1, 0, 60)
            encoder.prepareStreamEpoch(2)
            encoder.setCaptureProfile(original.copy(lensFacing = LensFacing.Front, targetFps = target.framesPerSecond))
            val second = facts.sample(encoder, 2, first, 30)
            assertEquals(LensFacing.Front, encoder.profile.lensFacing)
            assertTrue(encoder.captureSize.height >= 1920)
            encoder.restoreCommittedConfiguration(original, 1, bitrate)
            facts.sample(encoder, 1, second, 60)
            assertEquals(original, encoder.profile)
        } finally {
            encoder.close()
            encoder.cameraHandler.looper.thread.join(2_000)
            encoder.codecHandler.looper.thread.join(2_000)
        }
    }

    private class CaptureFacts : EncodedFrameListener {
        private data class Fact(val epoch: Int, val generation: Long, val pts: Long,
            val codec: NativeVideoCodec, val height: Int, val fps: Int)
        private val facts = java.util.concurrent.LinkedBlockingQueue<Fact>()
        override fun onEncodedFrame(frame: EncodedFrame) {
            facts.offer(Fact(frame.streamEpoch, frame.encoderGeneration, frame.presentationTimeUs,
                frame.configuration.codec, frame.configuration.height, frame.configuration.framesPerSecond))
        }
        fun sample(encoder: Camera2MediaEncoder, epoch: Int, afterGeneration: Long, requestedFps: Int): Long {
            val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(15)
            val samples = ArrayList<Fact>()
            val count = requestedFps * 4
            while (samples.size < count && System.nanoTime() < deadline) {
                val fact = facts.poll(500, TimeUnit.MILLISECONDS) ?: continue
                if (fact.generation <= afterGeneration) continue
                assertEquals("new generation must preserve its epoch", epoch, fact.epoch)
                assertEquals(NativeVideoCodec.Avc, fact.codec)
                assertEquals(1080, fact.height)
                assertEquals(requestedFps, fact.fps)
                samples += fact
            }
            assertEquals("capture failed: ${encoder.lastError}", count, samples.size)
            assertEquals(1, samples.map { it.generation }.distinct().size)
            val steady = samples.drop(requestedFps)
            assertTrue(steady.zipWithNext().all { (a, b) -> b.pts > a.pts })
            val fps = (steady.size - 1) * 1_000_000.0 / (steady.last().pts - steady.first().pts)
            Log.i("PicooCameraProbe", "lens=${encoder.profile.lensFacing}; orientation=${encoder.profile.displayRotationDegrees}; epoch=$epoch; generation=${samples.first().generation}; capture=${encoder.captureSize}; fps=$fps")
            assertTrue("actual fps=$fps", fps in requestedFps * 0.95..requestedFps * 1.05)
            return samples.first().generation
        }
    }

    private fun verify(codec: NativeVideoCodec, height: Int, fps: Int) {
        val profile = CaptureProfile(
            resolution = Size(if (height == 720) 1280 else 1920, height),
            targetFps = fps,
            lensFacing = LensFacing.Back,
            codec = codec,
            displayRotationDegrees = 90,
        )
        val targetCount = fps * 4
        val timestamps = ArrayList<Long>(targetCount)
        val errors = ArrayList<String>()
        val completed = CountDownLatch(1)
        val encoder = Camera2MediaEncoder(
            context = ApplicationProvider.getApplicationContext(),
            initialProfile = profile,
            initialBitrateBps = com.picoo.camera.jni.PicooNative.bitrateInitialForHeight(height),
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
