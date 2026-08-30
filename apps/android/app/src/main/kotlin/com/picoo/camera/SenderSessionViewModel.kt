package com.picoo.camera

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import com.picoo.camera.jni.PicooNative
import com.picoo.camera.media.Camera2MediaEncoder
import com.picoo.camera.media.EncodedFrameListener
import com.picoo.camera.media.EncoderReconfigurationCoordinator
import com.picoo.camera.media.ParameterSetsListener
import com.picoo.camera.media.StreamResolution
import com.picoo.camera.runtime.SenderNativeRuntime
import com.picoo.camera.ui.SenderHomeState
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference

/**
 * Configuration-stable owner for the Android Sender session.
 *
 * Rotation and window-size changes recreate the Activity, but must not destroy the Rust session or
 * camera encoder. Process-lifetime preferences are kept separately in SharedPreferences.
 */
class SenderSessionViewModel(application: Application) : AndroidViewModel(application) {
    private val preferences = application.getSharedPreferences(PREFERENCES_NAME, 0)

    val uiState = SenderHomeState().apply {
        autoConnectEnabled = preferences.getBoolean(KEY_AUTO_CONNECT, true)
        preferredResolutionLabel = preferences.getString(KEY_PREFERRED_RESOLUTION, null)
            ?.let(StreamResolution::fromLabel)
            ?.label
            ?: StreamResolution.P1080.label
    }
    val parameterSetsRef = AtomicReference<Pair<ByteArray, ByteArray>?>(null)
    val streamConfigDirty = AtomicBoolean(false)
    val runtime = SenderNativeRuntime(application)
    val encoderReconfiguration = EncoderReconfigurationCoordinator()
    val encoder = Camera2MediaEncoder(
        context = application,
        initialBitrateBps = PicooNative.bitrateInitialForHeight(StreamResolution.P720.height),
        initialStreamEpoch = PicooNative.readSenderSnapshot(runtime.senderHandle).streamEpoch,
        frameListener = EncodedFrameListener { data, isKeyFrame, ptsUs, streamEpoch ->
            val fragments = PicooNative.ingestAccessUnit(
                handle = runtime.senderHandle,
                data = data,
                keyframe = isKeyFrame,
                ptsUs = ptsUs,
                streamEpoch = streamEpoch,
            )
            if (fragments > 0) {
                PicooNative.flushPending(runtime.senderHandle)
                PicooNative.pump(runtime.senderHandle)
            }
        },
        parameterSetsListener = ParameterSetsListener { sps, pps ->
            parameterSetsRef.set(sps to pps)
            streamConfigDirty.set(true)
        },
    )

    fun setAutoConnectEnabled(enabled: Boolean) {
        uiState.autoConnectEnabled = enabled
        preferences.edit().putBoolean(KEY_AUTO_CONNECT, enabled).apply()
    }

    fun setPreferredResolution(resolution: StreamResolution) {
        uiState.preferredResolutionLabel = resolution.label
        preferences.edit().putString(KEY_PREFERRED_RESOLUTION, resolution.label).apply()
    }

    override fun onCleared() {
        encoder.close()
        runtime.close()
    }

    private companion object {
        const val PREFERENCES_NAME = "sender_settings"
        const val KEY_AUTO_CONNECT = "auto_connect_enabled"
        const val KEY_PREFERRED_RESOLUTION = "preferred_resolution"
    }
}
