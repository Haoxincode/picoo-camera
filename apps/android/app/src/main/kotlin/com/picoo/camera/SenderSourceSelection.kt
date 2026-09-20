package com.picoo.camera

import android.content.SharedPreferences
import android.hardware.camera2.CameraManager
import com.picoo.camera.jni.PicooNative
import com.picoo.camera.media.LensFacing
import com.picoo.camera.media.SourcePreparation
import com.picoo.camera.media.VideoSourceFormat
import com.picoo.camera.ui.SenderHomeState
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** ViewModel-owned preparation and persistent intent; never commits media (MEDIA-057). */
internal class SenderSourceSelection(
    private val manager: CameraManager,
    private val scope: CoroutineScope,
    private val state: SenderHomeState,
    private val preferences: SharedPreferences,
) {
    private var query: Pair<LensFacing, Int>? = null
    private var job: Job? = null

    init {
        state.preferredSourceFormat = VideoSourceFormat.fromWire(
            preferences.getInt("source_codec", 1),
            preferences.getInt("source_height", 1080),
            preferences.getInt("source_fps", 60),
        ) ?: VideoSourceFormat.Default
    }

    fun refresh(facing: LensFacing, rotation: Int) {
        val next = facing to rotation
        if (query == next) return
        query = next
        job?.cancel()
        state.localSourceFormats = null
        state.sourcePreparationError = null
        job = scope.launch {
            val result = withContext(Dispatchers.IO) {
                SourcePreparation.candidates(manager, facing, rotation, VideoSourceFormat.ProductFormats) {
                    PicooNative.bitrateInitialForHeight(it.resolution.height)
                }
            }
            state.localSourceFormats = result.getOrNull()
            state.sourcePreparationError = result.exceptionOrNull()?.let { "无法准备视频配置：${it.message}" }
        }
    }

    suspend fun prepareCameraSource(current: VideoSourceFormat, facing: LensFacing, rotation: Int): VideoSourceFormat? {
        val remote = state.receiverSourceFormats ?: return null
        val result = withContext(Dispatchers.IO) {
            SourcePreparation.candidates(manager, facing, rotation, remote) {
                PicooNative.bitrateInitialForHeight(it.resolution.height)
            }
        }
        val selected = com.picoo.camera.media.CameraSourceSelection.select(result.getOrNull().orEmpty(), current.codec)
        if (selected == null) {
            val lens = if (facing == LensFacing.Front) "前置镜头" else "后置镜头"
            state.errorText = "$lens 与接收端没有共同可用的视频格式，请调整手机方向后重试"
        }
        return selected
    }

    fun selectDefault(source: VideoSourceFormat) {
        if (!canPrepare(source)) return
        state.preferredSourceFormat = source
        preferences.edit().putInt("source_codec", source.codec.wireValue)
            .putInt("source_height", source.resolution.height)
            .putInt("source_fps", source.framesPerSecond).apply()
    }

    fun canPrepare(source: VideoSourceFormat): Boolean = admit(source, state.availableSourceFormats)

    private fun admit(source: VideoSourceFormat, candidates: List<VideoSourceFormat>?): Boolean {
        if (candidates?.contains(source) == true) return true
        state.errorText = state.sourcePreparationError ?: if (candidates == null) {
            "正在检查可用视频配置，请稍后重试"
        } else "当前镜头、方向或接收端不支持 ${source.label}，请选择可用配置"
        return false
    }
}
