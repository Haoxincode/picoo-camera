package com.picoo.camera.media

import android.content.Context
import android.hardware.camera2.CameraManager
import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** MEDIA-056: actual platform queries, no camera images or guessed capability products. */
class SourcePreparationContractTest {
    @Test fun preparationKeepsCompleteCandidatesAndExplicitLensAndOrientation() {
        assertTrue(com.picoo.camera.jni.PicooNative.ensureLoaded())
        val context = ApplicationProvider.getApplicationContext<Context>()
        val manager = context.getSystemService(CameraManager::class.java)
        val formats = NativeVideoCodec.entries.flatMap { codec ->
            StreamResolution.entries.flatMap { resolution ->
                listOf(30, 60).map { VideoSourceFormat(codec, resolution, it) }
            }
        }
        for (facing in LensFacing.entries) {
            if (CameraCapturePreparation.findCameraId(manager, facing) == null) continue
            for (rotation in listOf(0, 90)) {
                fun prepare(candidates: List<VideoSourceFormat>) = SourcePreparation.candidates(
                    manager, facing, rotation, candidates,
                ) { com.picoo.camera.jni.PicooNative.bitrateInitialForHeight(it.resolution.height) }.getOrThrow()
                val admitted = prepare(formats)
                assertTrue("$facing rotation=$rotation has no usable source", admitted.isNotEmpty())
                assertTrue(admitted.all { it in formats })
                assertEquals(emptyList<VideoSourceFormat>(), prepare(emptyList()))
                for (format in formats) {
                    val expected = if (format in admitted) listOf(format) else emptyList()
                    assertEquals("$facing rotation=$rotation $format", expected, prepare(listOf(format, format)))
                }
                android.util.Log.i("PicooPreparation", "$facing rotation=$rotation candidates=$admitted")
            }
        }
    }
}
