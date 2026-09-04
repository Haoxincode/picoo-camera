package com.picoo.camera

import android.app.Application
import android.widget.Toast
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.picoo.camera.jni.PicooNative
import com.picoo.camera.media.Camera2MediaEncoder
import com.picoo.camera.media.CaptureState
import com.picoo.camera.media.EncodedFrameListener
import com.picoo.camera.media.EncoderReconfigurationCoordinator
import com.picoo.camera.media.LensFacing
import com.picoo.camera.media.LinkQuality
import com.picoo.camera.media.LocalPreviewMirror
import com.picoo.camera.media.ParameterSetsListener
import com.picoo.camera.media.StreamResolution
import com.picoo.camera.runtime.QuicWifiBindingResult
import com.picoo.camera.runtime.SenderNativeRuntime
import com.picoo.camera.ui.SenderHomeState
import com.picoo.camera.ui.SenderTab
import com.picoo.camera.ui.screens.WaitOutcome
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

/**
 * Configuration-stable owner for the Android Sender session.
 *
 * Rotation and window-size changes recreate the Activity, but must not destroy the Rust session or
 * camera encoder. Process-lifetime preferences are kept separately in SharedPreferences.
 *
 * REQ-PICOO-UI-010: session pumping and encoder side effects live here; Compose observes.
 */
class SenderSessionViewModel(application: Application) : AndroidViewModel(application) {
    private val preferences = application.getSharedPreferences(PREFERENCES_NAME, 0)

    val uiState = SenderHomeState().apply {
        autoConnectEnabled = preferences.getBoolean(KEY_AUTO_CONNECT, true)
        preferredResolutionLabel = preferences.getString(KEY_PREFERRED_RESOLUTION, null)
            ?.let(StreamResolution::fromLabel)
            ?.label
            ?: StreamResolution.P1080.label
        manualEndpointText = preferences.getString(KEY_LAST_MANUAL_ENDPOINT, "").orEmpty()
    }
    val parameterSetsRef = AtomicReference<Pair<ByteArray, ByteArray>?>(null)
    val streamConfigDirty = AtomicBoolean(false)
    val runtime = SenderNativeRuntime(application)
    val encoderReconfiguration = EncoderReconfigurationCoordinator()
    private val encoderRef = AtomicReference<Camera2MediaEncoder?>(null)
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
            // Recovery control arrives on the reliable stream while video
            // keeps encoding. Consume the IDR request at frame cadence even
            // when this AU could not enter the bounded sender queue, so
            // recovery is not delayed until the 500ms maintenance fallback.
            if (PicooNative.takeKeyframeRequest(runtime.senderHandle) == 1) {
                encoderRef.get()?.requestKeyFrame()
            }
        },
        parameterSetsListener = ParameterSetsListener { sps, pps ->
            parameterSetsRef.set(sps to pps)
            streamConfigDirty.set(true)
        },
    )

    private var displayRotationDegrees: Int = 0
    private var cameraGranted: Boolean = false
    private var previousStatus: Int = PicooNative.STATUS_DISCONNECTED
    private var lastThermalAtMs: Long = 0L
    private var lastThermalStatus: Int? = null

    init {
        encoderRef.set(encoder)
        uiState.previewTransformInfo = encoder.previewTransformInfo
        val senderHandle = runtime.senderHandle
        if (senderHandle != 0L) {
            uiState.adaptiveBitrateBps = PicooNative.readSenderSnapshot(senderHandle).currentBitrateBps
        }
        viewModelScope.launch {
            while (isActive) {
                tick()
                delay(PUMP_INTERVAL_MS)
            }
        }
    }

    fun setAutoConnectEnabled(enabled: Boolean) {
        uiState.autoConnectEnabled = enabled
        preferences.edit().putBoolean(KEY_AUTO_CONNECT, enabled).apply()
    }

    fun setPreferredResolution(resolution: StreamResolution) {
        uiState.preferredResolutionLabel = resolution.label
        preferences.edit().putString(KEY_PREFERRED_RESOLUTION, resolution.label).apply()
    }

    /** Persist only the endpoint locator; pairing trust remains Rust-owned. */
    fun rememberManualEndpoint(host: String, port: Int) {
        val endpoint = "$host:$port"
        uiState.manualEndpointText = endpoint
        preferences.edit().putString(KEY_LAST_MANUAL_ENDPOINT, endpoint).apply()
    }

    fun setDisplayRotationDegrees(degrees: Int) {
        displayRotationDegrees = degrees
        encoder.setDisplayRotationDegrees(degrees)
    }

    fun applyStreamConfig() {
        encoder.setDisplayRotationDegrees(displayRotationDegrees)
        val width = encoder.profile.resolution.width
        val height = encoder.profile.resolution.height
        val rustBitrate = PicooNative.readSenderSnapshot(runtime.senderHandle).currentBitrateBps
        val bitrate = if (rustBitrate > 0) {
            rustBitrate
        } else {
            PicooNative.bitrateInitialForHeight(height)
        }
        val sets = parameterSetsRef.get()
        PicooNative.setStreamConfig(
            runtime.senderHandle,
            width = width,
            height = height,
            fps = 30,
            bitrateBps = bitrate,
            mirrored = uiState.remoteMirrored,
            // Android compositor already emits upright landscape pixels.
            rotation = 0,
            sps = sets?.first,
            pps = sets?.second,
        )
        streamConfigDirty.set(false)
    }

    fun beginLocalEncoderReconfiguration(targetHeight: Int): Boolean {
        val epoch = encoderReconfiguration.beginLocal(runtime.senderHandle, encoder, targetHeight)
        if (epoch == 0) {
            uiState.errorText = "正在完成上一项视频调整，请稍后重试"
            return false
        }
        return true
    }

    fun connect(host: String, port: Int, receiverId: String): Boolean {
        val ui = uiState
        ui.suppressAutoConnect = false
        ui.connectionStartedAtMs = 0L
        when (runtime.configureQuicWifiNetwork(host)) {
            QuicWifiBindingResult.Bound -> Unit
            QuicWifiBindingResult.WifiUnavailable -> {
                ui.errorText = "未连接 Wi-Fi，请连接电脑所在的 Wi-Fi 后重试"
                return false
            }
            QuicWifiBindingResult.Blocked -> {
                ui.errorText = "当前 VPN 阻止局域网连接，请关闭 VPN 或允许局域网访问后重试"
                return false
            }
        }
        val preferredResolution = StreamResolution.fromLabel(ui.preferredResolutionLabel)
        ui.resolutionLabel = preferredResolution.label
        val preferredBitrate = PicooNative.bitrateInitialForHeight(preferredResolution.height)
        encoder.setTargetBitrateBps(preferredBitrate)
        encoder.setResolution(preferredResolution.width, preferredResolution.height)
        PicooNative.setPreferredHeight(runtime.senderHandle, preferredResolution.height)
        applyStreamConfig()
        val rc = PicooNative.connect(runtime.senderHandle, host.trim(), port)
        if (rc == 0) {
            ui.lastShownSessionError = ""
            ui.selectedReceiverId = receiverId
            ui.connectionStartedAtMs = System.currentTimeMillis()
            val helloRc = PicooNative.sendClientHello(runtime.senderHandle)
            if (helloRc != 0) {
                PicooNative.disconnect(runtime.senderHandle)
                ui.connectionStartedAtMs = 0L
                ui.errorText = "无法开始安全连接 ($helloRc)"
                return false
            }
            ui.errorText = null
            val startedAt = ui.connectionStartedAtMs
            viewModelScope.launch {
                delay(CONNECT_TIMEOUT_MS)
                if (ui.connectionStartedAtMs == startedAt &&
                    ui.senderStatus in setOf(
                        PicooNative.STATUS_CONNECTING,
                        PicooNative.STATUS_NEGOTIATING,
                        PicooNative.STATUS_RECONNECTING,
                    )
                ) {
                    PicooNative.disconnect(runtime.senderHandle)
                    ui.connectionStartedAtMs = 0L
                    ui.errorText = "无法连接到 ${ui.hostText}:${ui.portText}，请确认电脑端已打开且位于同一局域网"
                }
            }
            return true
        } else {
            ui.senderStatus = PicooNative.readSenderSnapshot(runtime.senderHandle).status
            ui.errorText = if (rc == -3) {
                "当前 VPN 不允许局域网连接，请允许局域网访问或关闭 VPN 后重试"
            } else {
                "连接失败 ($rc)"
            }
            return false
        }
    }

    fun onCameraGrantedChanged(granted: Boolean) {
        cameraGranted = granted
        reconcileCapture()
    }

    fun reloadTrustedStore(): Boolean {
        val devices = runtime.reloadTrustedDevices() ?: run {
            uiState.errorText = "无法读取已配对设备，已保留上一次有效数据"
            return false
        }
        uiState.pairedDevices = devices
        return true
    }

    fun resetToDevices() {
        uiState.senderTab = SenderTab.Devices
        uiState.phonePairingConfirmed = false
        uiState.pairingExpired = false
        uiState.pairingRemainingSeconds = 60
        uiState.waitOutcome = WaitOutcome.Pending
        uiState.errorText = null
    }

    fun regeneratePairing() {
        uiState.pairingExpired = false
        uiState.pairingRemainingSeconds = 60
        uiState.phonePairingConfirmed = false
        uiState.waitOutcome = WaitOutcome.Pending
        PicooNative.disconnect(runtime.senderHandle)
        val port = uiState.portText.toIntOrNull() ?: 4433
        connect(uiState.hostText, port, uiState.selectedReceiverId)
    }

    fun isLiveSession(status: Int = uiState.senderStatus): Boolean = when (status) {
        PicooNative.STATUS_STREAMING,
        PicooNative.STATUS_RECONNECTING,
        PicooNative.STATUS_NETWORK_UNSTABLE,
        -> true
        else -> false
    }

    private fun tick() {
        val senderHandle = runtime.senderHandle
        val ui = uiState
        if (senderHandle != 0L) {
            if (ui.senderStatus == PicooNative.STATUS_RECONNECTING) {
                // A replacement Wi-Fi network has a new Android Network handle. Refresh the
                // route before Rust creates its next automatic-reconnect socket.
                runtime.configureQuicWifiNetwork(ui.hostText)
            }
            PicooNative.pump(senderHandle)
            var senderSnapshot = PicooNative.readSenderSnapshot(senderHandle)
            ui.senderStatus = senderSnapshot.status
            if (ui.senderStatus == PicooNative.STATUS_PAIRING ||
                ui.senderStatus == PicooNative.STATUS_STREAMING ||
                ui.senderStatus == PicooNative.STATUS_NETWORK_UNSTABLE
            ) {
                ui.connectionStartedAtMs = 0L
            }
            if (ui.senderStatus == PicooNative.STATUS_RECONNECTING) {
                ui.reconnectAttempt = senderSnapshot.reconnectAttempt
                ui.reconnectDelayMs = senderSnapshot.reconnectDelayMs
            } else {
                ui.reconnectAttempt = 0
                ui.reconnectDelayMs = 0L
            }
            ui.encoderState = encoder.state
            val latestPreviewTransformInfo = encoder.previewTransformInfo
            if (latestPreviewTransformInfo != ui.previewTransformInfo) {
                ui.previewTransformInfo = latestPreviewTransformInfo
            }
            if (ui.senderStatus == PicooNative.STATUS_DISCONNECTED) {
                encoderReconfiguration.abandonDisconnectedSession(senderHandle)
            }
            when (val result = encoderReconfiguration.poll(senderHandle, encoder)) {
                is EncoderReconfigurationCoordinator.PollResult.Failed -> {
                    streamConfigDirty.set(false)
                    senderSnapshot = PicooNative.readSenderSnapshot(senderHandle)
                    ui.resolutionLabel =
                        StreamResolution.fromHeight(senderSnapshot.activeHeight).label
                    ui.errorText = result.message
                }
                is EncoderReconfigurationCoordinator.PollResult.Applied -> {
                    senderSnapshot = PicooNative.readSenderSnapshot(senderHandle)
                    ui.adaptiveBitrateBps = result.bitrateBps
                    ui.resolutionLabel = StreamResolution.fromHeight(result.actualHeight).label
                    encoder.setTargetBitrateBps(ui.adaptiveBitrateBps)
                    streamConfigDirty.set(true)
                    encoder.requestKeyFrame()
                }
                is EncoderReconfigurationCoordinator.PollResult.Recovered -> {
                    senderSnapshot = PicooNative.readSenderSnapshot(senderHandle)
                    ui.adaptiveBitrateBps = result.bitrateBps
                    ui.resolutionLabel = StreamResolution.fromHeight(result.actualHeight).label
                    encoder.setTargetBitrateBps(ui.adaptiveBitrateBps)
                    streamConfigDirty.set(true)
                    encoder.requestKeyFrame()
                    ui.errorText = "${result.message}；已恢复上一视频配置"
                }
                null -> Unit
            }
            if (!encoderReconfiguration.isPending &&
                encoder.appliedStreamEpoch == senderSnapshot.streamEpoch &&
                encoder.appliedEncoderHeight > 0 &&
                encoder.appliedEncoderHeight != senderSnapshot.activeHeight
            ) {
                val actualHeight = encoder.appliedEncoderHeight
                val reportResult = PicooNative.reportEncoderHeight(
                    senderHandle,
                    actualHeight,
                    encoder.appliedStreamEpoch,
                )
                if (reportResult == 0) {
                    senderSnapshot = PicooNative.readSenderSnapshot(senderHandle)
                    ui.resolutionLabel = StreamResolution.fromHeight(actualHeight).label
                    streamConfigDirty.set(true)
                } else if (beginLocalEncoderReconfiguration(actualHeight)) {
                    val actualSize = encoder.profile.resolution
                    encoder.setResolution(actualSize.width, actualSize.height)
                }
            }
            ui.pairingCode = PicooNative.getPairingShortCode(senderHandle)
            ui.connectedReceiverId = PicooNative.getConnectedReceiverId(senderHandle)
            ui.connectedReceiverName =
                PicooNative.getConnectedReceiverDisplayName(senderHandle)
            if (ui.connectedReceiverName.isNotEmpty()) {
                ui.selectedReceiverName = ui.connectedReceiverName
            }
            val bps = senderSnapshot.currentBitrateBps
            if (bps > 0) {
                ui.adaptiveBitrateBps = bps
                encoder.setTargetBitrateBps(bps)
            }
            val link = PicooNative.getLinkStats(senderHandle)
            ui.linkQualityChip = if (link != null && link.size >= 6) {
                LinkQuality.formatChip(rttMs = link[0], packetLoss = link[1])
            } else {
                ""
            }
            if (PicooNative.takeKeyframeRequest(senderHandle) == 1) {
                encoder.requestKeyFrame()
            }
            // PAIRING-001/004: only an explicit receiver signal is a rejection.
            val sessionErr = PicooNative.lastSessionError(senderHandle)
            if (sessionErr.isNotEmpty() && sessionErr != ui.lastShownSessionError) {
                ui.lastShownSessionError = sessionErr
                val msg = when (sessionErr) {
                    "PAIRING_REJECTED" -> "电脑端拒绝了连接"
                    "PUBLIC_KEY_CHANGED" ->
                        "电脑端检测到公钥变化，已拒绝自动连接。请删除配对后重新核对短码。"
                    "UNPAIRED" ->
                        "尚未完成配对，无法开始推流。请先完成六位短码确认。"
                    else -> "会话错误：$sessionErr"
                }
                ui.errorText = msg
                if (sessionErr == "PAIRING_REJECTED") {
                    ui.waitOutcome = WaitOutcome.Rejected
                    ui.senderTab = SenderTab.Wait
                } else {
                    Toast.makeText(getApplication(), msg, Toast.LENGTH_LONG).show()
                }
                if (sessionErr == "PUBLIC_KEY_CHANGED") {
                    ui.senderTab = SenderTab.Devices
                }
            }
            run {
                val camOut = IntArray(3)
                when (PicooNative.takeCameraCommand(senderHandle, camOut)) {
                    1 -> {
                        if (encoder.profile.lensFacing != LensFacing.Front &&
                            beginLocalEncoderReconfiguration(encoder.profile.resolution.height)
                        ) {
                            encoder.setLensFacing(LensFacing.Front)
                            ui.localPreviewMirrored =
                                LocalPreviewMirror.defaultFor(encoder.profile.lensFacing)
                            streamConfigDirty.set(true)
                        }
                    }
                    2 -> {
                        if (encoder.profile.lensFacing != LensFacing.Back &&
                            beginLocalEncoderReconfiguration(encoder.profile.resolution.height)
                        ) {
                            encoder.setLensFacing(LensFacing.Back)
                            ui.localPreviewMirrored =
                                LocalPreviewMirror.defaultFor(encoder.profile.lensFacing)
                            streamConfigDirty.set(true)
                        }
                    }
                    3 -> {
                        val w = camOut[0]
                        val h = camOut[1]
                        if (w > 0 && h > 0) {
                            val res = StreamResolution.fromHeight(h)
                            if (beginLocalEncoderReconfiguration(res.height)) {
                                ui.resolutionLabel = res.label
                                val bitrate = PicooNative.bitrateInitialForHeight(res.height)
                                encoder.setTargetBitrateBps(bitrate)
                                encoder.setResolution(res.width, res.height)
                                PicooNative.setPreferredHeight(senderHandle, res.height)
                                streamConfigDirty.set(true)
                                encoder.requestKeyFrame()
                            }
                        }
                    }
                    4 -> {
                        ui.remoteMirrored = camOut[2] != 0
                        streamConfigDirty.set(true)
                    }
                    5 -> {
                        if (beginLocalEncoderReconfiguration(encoder.profile.resolution.height)) {
                            encoder.switchCamera()
                            ui.localPreviewMirrored =
                                LocalPreviewMirror.defaultFor(encoder.profile.lensFacing)
                            streamConfigDirty.set(true)
                        }
                    }
                }
            }
            val receiverMaxHeight = senderSnapshot.receiverMaxHeight
            if (!encoderReconfiguration.isPending &&
                receiverMaxHeight in 1 until encoder.profile.resolution.height
            ) {
                val target = StreamResolution.fromHeight(receiverMaxHeight)
                val targetBitrate = PicooNative.bitrateInitialForHeight(target.height)
                if (beginLocalEncoderReconfiguration(target.height)) {
                    ui.resolutionLabel = target.label
                    encoder.setTargetBitrateBps(targetBitrate)
                    encoder.setResolution(target.width, target.height)
                    streamConfigDirty.set(true)
                    encoder.requestKeyFrame()
                }
            }
            if (!encoderReconfiguration.isPending) {
                val directive = PicooNative.readEncoderDirective(senderHandle)
                if (directive != null) {
                    val maxH = senderSnapshot.receiverMaxHeight
                    val thermalBlocks = ui.thermalForced720 && directive.targetHeight > 720
                    val capabilityBlocks = maxH in 1 until directive.targetHeight
                    if (thermalBlocks || capabilityBlocks) {
                        PicooNative.nackEncoderDirective(senderHandle, directive.id)
                    } else {
                        val next = StreamResolution.fromHeight(directive.targetHeight)
                        if (encoderReconfiguration.beginDirective(
                                senderHandle,
                                encoder,
                                directive,
                            )
                        ) {
                            ui.resolutionLabel = next.label
                            encoder.setTargetBitrateBps(directive.targetBitrateBps)
                            encoder.setResolution(next.width, next.height)
                        }
                    }
                }
            }
            if (previousStatus == PicooNative.STATUS_RECONNECTING &&
                ui.senderStatus == PicooNative.STATUS_STREAMING
            ) {
                if (cameraGranted &&
                    beginLocalEncoderReconfiguration(encoder.profile.resolution.height)
                ) {
                    encoder.stopPreview()
                    encoder.startPreview()
                    streamConfigDirty.set(true)
                }
            }
            if (ui.phonePairingConfirmed &&
                ui.senderStatus == PicooNative.STATUS_STREAMING &&
                previousStatus != PicooNative.STATUS_STREAMING
            ) {
                reloadTrustedStore()
            }
            if (ui.senderStatus == PicooNative.STATUS_DISCONNECTED &&
                previousStatus != PicooNative.STATUS_DISCONNECTED &&
                !ui.suppressAutoConnect
            ) {
                runtime.clearAutoConnectAttempts()
            }
            previousStatus = ui.senderStatus
            if (!encoderReconfiguration.isPending &&
                streamConfigDirty.get() &&
                (ui.senderStatus == PicooNative.STATUS_STREAMING ||
                    ui.senderStatus == PicooNative.STATUS_NEGOTIATING)
            ) {
                applyStreamConfig()
            }
        }
        reconcileCapture()
        applyThermalPolicy()
    }

    private fun reconcileCapture() {
        val liveSession = isLiveSession()
        val senderHandle = runtime.senderHandle
        if (liveSession && !cameraGranted) {
            if (senderHandle != 0L) PicooNative.markPermissionRequired(senderHandle)
        } else if (liveSession) {
            if (senderHandle != 0L) PicooNative.clearPermissionRequired(senderHandle)
            encoder.startPreview()
            if (senderHandle != 0L && PicooNative.takeKeyframeRequest(senderHandle) == 1) {
                encoder.requestKeyFrame()
            }
            uiState.encoderState = encoder.state
        } else if (encoder.state == CaptureState.Opening ||
            encoder.state == CaptureState.Previewing
        ) {
            encoder.stopPreview()
            uiState.encoderState = encoder.state
        }
    }

    private fun applyThermalPolicy() {
        val ui = uiState
        val senderHandle = runtime.senderHandle
        val live = isLiveSession()
        if (!live) {
            ui.powerHint = ""
            ui.thermalForced720 = false
            ui.thermalToastShown = false
            lastThermalAtMs = 0L
            lastThermalStatus = ui.senderStatus
            return
        }
        if (lastThermalStatus != ui.senderStatus) {
            lastThermalAtMs = 0L
            lastThermalStatus = ui.senderStatus
        }
        val now = System.currentTimeMillis()
        if (lastThermalAtMs != 0L && now - lastThermalAtMs < THERMAL_INTERVAL_MS) {
            return
        }
        lastThermalAtMs = now
        val context = getApplication<Application>()
        val thermal = PowerHints.readThermalStatus(context)
        ui.powerHint = PowerHints.readHint(context)
        val force720 = PowerHints.shouldForce720p(thermal)
        ui.thermalForced720 = force720
        if (force720 && !ui.thermalToastShown) {
            // AC-M-LIVE-02: toast when thermal throttle engages (banner remains visible).
            Toast.makeText(
                context,
                "设备偏热保护中 · 已降至 720p，1080P 暂不可选",
                Toast.LENGTH_SHORT,
            ).show()
            ui.thermalToastShown = true
        } else if (!force720) {
            ui.thermalToastShown = false
        }
        if (senderHandle != 0L) {
            PicooNative.setThermalHold(senderHandle, force720)
        }
        if (force720 && ui.resolutionLabel == "1080p") {
            val targetBitrate = PicooNative.bitrateInitialForHeight(720)
            if (beginLocalEncoderReconfiguration(720)) {
                ui.resolutionLabel = "720p"
                encoder.setTargetBitrateBps(targetBitrate)
                encoder.setResolution(1280, 720)
                streamConfigDirty.set(true)
                encoder.requestKeyFrame()
            }
        }
    }

    override fun onCleared() {
        encoder.close()
        runtime.close()
    }

    private companion object {
        const val PREFERENCES_NAME = "sender_settings"
        const val KEY_AUTO_CONNECT = "auto_connect_enabled"
        const val KEY_PREFERRED_RESOLUTION = "preferred_resolution"
        const val KEY_LAST_MANUAL_ENDPOINT = "last_manual_endpoint"
        const val PUMP_INTERVAL_MS = 500L
        const val CONNECT_TIMEOUT_MS = 10_000L
        const val THERMAL_INTERVAL_MS = 5_000L
    }
}
