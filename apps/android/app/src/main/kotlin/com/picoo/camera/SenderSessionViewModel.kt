package com.picoo.camera

import android.app.Application
import android.os.Handler
import android.os.HandlerThread
import android.widget.Toast
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.picoo.camera.discovery.PairedAutoConnect
import com.picoo.camera.jni.PicooNative
import com.picoo.camera.media.Camera2MediaEncoder
import com.picoo.camera.media.CaptureState
import com.picoo.camera.media.EncodedFrameListener
import com.picoo.camera.media.EncodedAccessUnitBuffer
import com.picoo.camera.media.EncodedAccessUnitHandoff
import com.picoo.camera.media.EncoderSubmitOutcome
import com.picoo.camera.media.EncoderReconfigurationCoordinator
import com.picoo.camera.media.LensFacing
import com.picoo.camera.media.LinkQuality
import com.picoo.camera.media.LocalPreviewMirror
import com.picoo.camera.media.EncodedFrameConfiguration
import com.picoo.camera.media.StreamResolution
import com.picoo.camera.media.VideoSourceFormat
import com.picoo.camera.runtime.QuicWifiBindingResult
import com.picoo.camera.runtime.SenderNativeRuntime
import com.picoo.camera.ui.SenderHomeState
import com.picoo.camera.ui.SenderTab
import com.picoo.camera.ui.screens.WaitOutcome
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference
import kotlinx.coroutines.delay
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

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
        manualEndpointText = preferences.getString(KEY_LAST_MANUAL_ENDPOINT, "").orEmpty()
    }
    val streamConfigDirty = AtomicBoolean(false)
    private val configurationKeyframeRequested = AtomicBoolean(false)
    private val remoteMirroredRef = AtomicBoolean(false)
    val runtime = SenderNativeRuntime(application)
    val encoderReconfiguration = EncoderReconfigurationCoordinator()
    private val encoderRef = AtomicReference<Camera2MediaEncoder?>(null)
    private val senderMediaThread = HandlerThread("picoo-sender-media").apply { start() }
    private val senderMediaHandler = Handler(senderMediaThread.looper)
    private val encodedAccessUnits = EncodedAccessUnitBuffer()
    // Owned only by the serial media worker; UI dirty flags cannot acknowledge
    // a new encoder generation whose IDR has not reached Core yet.
    private var configuredMediaGeneration = 0L
    private var configuredMediaSource: EncodedFrameConfiguration? = null
    val encoder = Camera2MediaEncoder(
        context = application,
        initialBitrateBps = PicooNative.bitrateInitialForHeight(StreamResolution.P720.height),
        initialStreamEpoch = PicooNative.readSenderSnapshot(runtime.senderHandle).streamEpoch,
        frameListener = EncodedFrameListener { frame ->
            enqueueEncodedAccessUnit(EncodedAccessUnitHandoff(
                data = frame.data, isKeyFrame = frame.isKeyFrame,
                presentationTimeUs = frame.presentationTimeUs, encodedAtUs = frame.encodedAtUs,
                streamEpoch = frame.streamEpoch, encoderGeneration = frame.encoderGeneration,
                configuration = frame.configuration, enqueuedAtNanos = System.nanoTime(),
            ))
        },

    )

    private val sourceSelection = SenderSourceSelection(encoder.cameraManager, viewModelScope, uiState, preferences)
    private val pendingConnectionSource = AtomicReference<VideoSourceFormat?>(null)
    private var requestedDisplayRotationDegrees: Int = 0
    private var pendingDisplayRotation: Int? = null
    private var cameraGranted: Boolean = false
    private var previousStatus: Int = PicooNative.STATUS_DISCONNECTED
    private var thermalWarningShown = false
    private var lastThermalAtMs: Long = 0L
    private var lastThermalStatus: Int? = null

    init {
        encoderRef.set(encoder)
        sourceSelection.refresh(encoder.profile.lensFacing, requestedDisplayRotationDegrees)
        uiState.previewTransformInfo = encoder.previewTransformInfo
        val senderHandle = runtime.senderHandle
        if (senderHandle != 0L) {
            uiState.adaptiveBitrateBps = PicooNative.readSenderSnapshot(senderHandle).currentBitrateBps
        }
        if (senderHandle != 0L) {
            viewModelScope.launch {
                var eventRevision = 0L
                while (isActive) {
                    eventRevision = withContext(Dispatchers.IO) {
                        PicooNative.waitForSenderEvent(
                            senderHandle,
                            eventRevision,
                            MAINTENANCE_TIMEOUT_MS,
                        )
                    }
                    if (!isActive) break
                    tick()
                }
            }
        }
    }

    private fun enqueueEncodedAccessUnit(accessUnit: EncodedAccessUnitHandoff) {
        val offer = encodedAccessUnits.offer(accessUnit, System.nanoTime())
        if (offer.scheduleWorker) {
            senderMediaHandler.post(::drainEncodedAccessUnits)
        }
    }

    private fun drainEncodedAccessUnits() {
        while (true) {
            val work = encodedAccessUnits.take(System.nanoTime()) ?: return
            if (work.recoveryRequired) {
                encoderRef.get()?.requestKeyFrame()
            }
            val accessUnit = work.accessUnit ?: continue
            submitEncodedAccessUnit(accessUnit)
        }
    }

    private fun submitEncodedAccessUnit(accessUnit: EncodedAccessUnitHandoff) {
        if (pendingConnectionSource.get() != null) return
        val configuration = accessUnit.configuration
        val configureStream = accessUnit.isKeyFrame && (
            streamConfigDirty.getAndSet(false) ||
                configuredMediaGeneration != accessUnit.encoderGeneration ||
                configuredMediaSource !== configuration
            )
        val outcome = PicooNative.submitEncoderAccessUnit(
            handle = runtime.senderHandle,
            data = accessUnit.data,
            keyframe = accessUnit.isKeyFrame,
            ptsUs = accessUnit.presentationTimeUs,
            encodedAtUs = accessUnit.encodedAtUs,
            streamEpoch = accessUnit.streamEpoch,
            encoderGeneration = accessUnit.encoderGeneration,
            encoderWidth = accessUnit.configuration.width,
            encoderHeight = accessUnit.configuration.height,
            configureStream = configureStream,
            mirrored = remoteMirroredRef.get(),
            codec = configuration.codec.wireValue,
            fps = configuration.framesPerSecond,
            codecConfiguration = if (configureStream) configuration.record else null,
        )
        if (outcome is EncoderSubmitOutcome.Error) {
            if (configureStream) {
                streamConfigDirty.set(true)
                configurationKeyframeRequested.set(false)
            }
            return
        }
        if (outcome is EncoderSubmitOutcome.Rejected) {
            if (configureStream) streamConfigDirty.set(true)
            if (outcome.keyframeRequested) encoderRef.get()?.requestKeyFrame()
            return
        }
        outcome as EncoderSubmitOutcome.Accepted
        if (configureStream && !outcome.streamConfigured) {
            streamConfigDirty.set(true)
        }
        if (outcome.streamConfigured) {
            configuredMediaGeneration = accessUnit.encoderGeneration
            configuredMediaSource = configuration
            configurationKeyframeRequested.set(false)
        }
        encoder.recordAcceptedFrame(
            accessUnit.data.size,
            accessUnit.isKeyFrame,
            accessUnit.streamEpoch,
            accessUnit.configuration.height,
        )
        if (outcome.keyframeRequested) {
            encoderRef.get()?.requestKeyFrame()
        }
    }

    fun setAutoConnectEnabled(enabled: Boolean) {
        uiState.autoConnectEnabled = enabled
        preferences.edit().putBoolean(KEY_AUTO_CONNECT, enabled).apply()
    }

    fun setPreferredSourceFormat(source: VideoSourceFormat) = sourceSelection.selectDefault(source)

    fun requestSourceFormat(source: VideoSourceFormat): Boolean {
        if (!cameraGranted) {
            uiState.errorText = "请先允许摄像头权限"
            return false
        }
        if (PicooNative.readSenderSnapshot(runtime.senderHandle).status !in setOf(
                PicooNative.STATUS_STREAMING, PicooNative.STATUS_NETWORK_UNSTABLE,
            )
        ) {
            uiState.errorText = "请先完成与接收端的连接"
            return false
        }
        if (!sourceSelection.canPrepare(source)) return false
        val epoch = encoderReconfiguration.beginLocal(runtime.senderHandle, encoder, source)
        if (epoch == 0) {
            uiState.errorText = "正在完成上一项视频调整，请稍后重试"
            return false
        }
        val bitrate = PicooNative.bitrateInitialForHeight(source.resolution.height)
        PicooNative.setPreferredHeight(runtime.senderHandle, source.resolution.height)
        encoder.setTargetBitrateBps(bitrate)
        encoder.setDisplayRotationDegrees(requestedDisplayRotationDegrees)
        pendingDisplayRotation = null
        encoder.setSourceFormat(source)
        applyStreamConfig()
        if (cameraGranted) encoder.startPreview()
        pendingConnectionSource.set(null)
        uiState.errorText = null
        return true
    }

    /** Persist only the endpoint locator; pairing trust remains Rust-owned. */
    fun rememberManualEndpoint(host: String, port: Int) {
        val endpoint = "$host:$port"
        uiState.manualEndpointText = endpoint
        preferences.edit().putString(KEY_LAST_MANUAL_ENDPOINT, endpoint).apply()
    }

    fun setDisplayRotationDegrees(degrees: Int) {
        val normalized = ((degrees % 360) + 360) % 360
        require(normalized in listOf(0, 90, 180, 270))
        requestedDisplayRotationDegrees = normalized
        sourceSelection.refresh(encoder.profile.lensFacing, normalized)
        if (!isLiveSession() || pendingConnectionSource.get() != null) {
            encoder.setDisplayRotationDegrees(normalized)
            pendingDisplayRotation = null
        } else if (encoder.displayRotationDegrees != normalized) {
            pendingDisplayRotation = normalized
        } else {
            pendingDisplayRotation = null
        }
    }

    fun applyStreamConfig() {
        // Configuration commits belong to native AU submission, never a UI-side
        // setter that can overtake queued frames from the previous generation.
        streamConfigDirty.set(true)
        if (configurationKeyframeRequested.compareAndSet(false, true)) {
            encoder.requestKeyFrame()
        }
    }

    fun beginLocalEncoderReconfiguration(targetHeight: Int): Boolean {
        val resolution = StreamResolution.fromHeight(targetHeight) ?: return false
        val source = VideoSourceFormat(encoder.profile.codec, resolution, encoder.profile.targetFps)
        val epoch = encoderReconfiguration.beginLocal(runtime.senderHandle, encoder, source)
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
        val requested = ui.preferredSourceFormat
        if (!sourceSelection.canPrepareLocally(requested)) return false
        encoderReconfiguration.abandonDisconnectedSession()
        pendingConnectionSource.set(requested)
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
            pendingConnectionSource.set(null)
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
        pendingConnectionSource.set(null)
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
            ui.committedSourceFormat = senderSnapshot.lastCommittedSourceFormat
            ui.receiverSourceFormats = senderSnapshot.receiverSourceFormats
            sourceSelection.refresh(encoder.profile.lensFacing, requestedDisplayRotationDegrees)
            val requested = pendingConnectionSource.get()
            if (requested != null && ui.sourcePreparationError != null) {
                ui.errorText = ui.sourcePreparationError
                PicooNative.disconnect(senderHandle)
                pendingConnectionSource.set(null)
                return
            }
            if (requested != null && cameraGranted && senderSnapshot.receiverSourceFormats != null &&
                ui.localSourceFormats != null && ui.senderStatus in setOf(
                    PicooNative.STATUS_STREAMING, PicooNative.STATUS_NETWORK_UNSTABLE,
                )
            ) {
                // Keep fresh offers available for an explicit alternative. The
                // pending intent continues to block old preview AUs meanwhile.
                if (!requestSourceFormat(requested)) return
                pendingConnectionSource.set(null)
            }
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
                pendingConnectionSource.set(null)
                encoderReconfiguration.abandonDisconnectedSession()
            }
            when (val result = encoderReconfiguration.poll(senderHandle, encoder)) {
                is EncoderReconfigurationCoordinator.PollResult.Failed -> {
                    streamConfigDirty.set(false)
                    senderSnapshot = PicooNative.readSenderSnapshot(senderHandle)
                    ui.committedSourceFormat = senderSnapshot.lastCommittedSourceFormat
                    ui.errorText = result.message
                }
                is EncoderReconfigurationCoordinator.PollResult.Applied -> {
                    senderSnapshot = PicooNative.readSenderSnapshot(senderHandle)
                    ui.adaptiveBitrateBps = result.bitrateBps
                    ui.committedSourceFormat = result.actualFormat
                    encoder.setTargetBitrateBps(ui.adaptiveBitrateBps)
                }
                is EncoderReconfigurationCoordinator.PollResult.Recovered -> {
                    senderSnapshot = PicooNative.readSenderSnapshot(senderHandle)
                    ui.adaptiveBitrateBps = result.bitrateBps
                    ui.committedSourceFormat = result.actualFormat
                    encoder.setTargetBitrateBps(ui.adaptiveBitrateBps)
                    ui.errorText = "${result.message}；已恢复上一视频配置"
                }
                null -> Unit
            }
            if (pendingDisplayRotation != null && !encoderReconfiguration.isPending &&
                ui.availableSourceFormats != null && cameraGranted &&
                ui.senderStatus in setOf(PicooNative.STATUS_STREAMING, PicooNative.STATUS_NETWORK_UNSTABLE)
            ) {
                // Consume this orientation intent once. A rejected pose needs
                // another user action, not a retry/recovery loop on every tick.
                pendingDisplayRotation = null
                senderSnapshot.lastCommittedSourceFormat?.let(::requestSourceFormat)
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
                if (!encoderReconfiguration.isPending) encoder.setTargetBitrateBps(bps)
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
                        val res = StreamResolution.fromHeight(h)
                        if (res != null && w == res.width) {
                            requestSourceFormat(VideoSourceFormat(encoder.profile.codec, res, encoder.profile.targetFps))
                        } else {
                            ui.errorText = "电脑请求的视频尺寸不受支持"
                        }
                    }
                    4 -> {
                        ui.remoteMirrored = camOut[2] != 0
                        remoteMirroredRef.set(ui.remoteMirrored)
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
            if (!encoderReconfiguration.isPending) {
                val directive = PicooNative.readEncoderDirective(senderHandle)
                if (directive != null) {
                    encoderReconfiguration.beginDirective(senderHandle, encoder, directive)
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
            if (!ui.suppressAutoConnect &&
                PairedAutoConnect.shouldSuppressAfterRemoteStop(
                    previousStatus = previousStatus,
                    currentStatus = ui.senderStatus,
                )
            ) {
                // REQ-PICOO-SESSION-009: Rust has already cleared its reconnect
                // intent. Keep NSD auto-connect from creating a brand-new session
                // to the Receiver that just asked us to stop.
                ui.suppressAutoConnect = true
                resetToDevices()
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
        if (pendingConnectionSource.get() != null) {
            if (cameraGranted) PicooNative.clearPermissionRequired(runtime.senderHandle)
            else PicooNative.markPermissionRequired(runtime.senderHandle)
            return
        }
        val liveSession = isLiveSession()
        val senderHandle = runtime.senderHandle
        if (liveSession && !cameraGranted) {
            if (senderHandle != 0L) PicooNative.markPermissionRequired(senderHandle)
        } else if (liveSession) {
            if (senderHandle != 0L) PicooNative.clearPermissionRequired(senderHandle)
            if (encoder.state == CaptureState.Idle && !encoderReconfiguration.isPending) {
                if (!beginLocalEncoderReconfiguration(encoder.profile.resolution.height)) return
                streamConfigDirty.set(true)
            }
            if (encoder.state == CaptureState.Error) return
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
            ui.thermalLimited = false
            thermalWarningShown = false
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
        val holdBitrate = PowerHints.shouldHoldBitrateGrowth(thermal)
        ui.thermalLimited = holdBitrate
        if (holdBitrate && !thermalWarningShown) {
            // AC-M-LIVE-02: toast when thermal throttle engages (banner remains visible).
            Toast.makeText(
                context,
                "设备温度较高，请注意散热或停止推流",
                Toast.LENGTH_SHORT,
            ).show()
            thermalWarningShown = true
        } else if (!holdBitrate) {
            thermalWarningShown = false
        }
        if (senderHandle != 0L) {
            PicooNative.setThermalHold(senderHandle, holdBitrate)
        }
    }

    override fun onCleared() {
        encoder.close()
        encodedAccessUnits.close()
        senderMediaThread.quitSafely()
        runCatching { senderMediaThread.join(1_000) }
        runtime.close()
    }

    private companion object {
        const val PREFERENCES_NAME = "sender_settings"
        const val KEY_AUTO_CONNECT = "auto_connect_enabled"
        const val KEY_LAST_MANUAL_ENDPOINT = "last_manual_endpoint"
        const val MAINTENANCE_TIMEOUT_MS = 500
        const val CONNECT_TIMEOUT_MS = 10_000L
        const val THERMAL_INTERVAL_MS = 5_000L
    }
}
