import Observation
import SwiftUI

// REQ-PICOO-UI-010: SwiftUI observes snapshots and sends actions; Rust owns
// the protocol/session state machine, AVFoundation owns camera lifecycle.
@MainActor
@Observable
final class SenderAppModel {
    private(set) var screen: SenderScreen = .devices
    private(set) var receiverSourceFormats: [VideoSourceFormat]?
    private(set) var receivers: [ReceiverSummary] = []
    private(set) var isDiscovering = true
    private(set) var senderStatus: PicooSenderStatus = .disconnected
    private(set) var pairingCode = ""
    private(set) var pairingSecondsRemaining = 60
    private(set) var receiverName = "电脑"
    private(set) var receiverEndpoint: ReceiverEndpoint?
    private(set) var errorMessage: String?
    private(set) var isConnecting = false
    private(set) var phoneConfirmedPairing = false
    private(set) var stopArmed = false
    private(set) var remoteMirrored = false
    private(set) var activeBitrateBps: UInt32
    private(set) var trustedReceivers: [TrustedReceiverSummary] = []
    private(set) var pairingWaitOutcome: PairingWaitOutcome = .pending
    private(set) var reconnectAttempt: UInt32 = 0
    private(set) var reconnectDelayMs: UInt64 = 0

    var manualEndpointText = ""
    var isManualConnectPresented = false
    var isSettingsPresented = false
    var autoConnectEnabled: Bool {
        didSet {
            defaults.set(autoConnectEnabled, forKey: Self.autoConnectPreferenceKey)
            discovery.resetAutoConnect()
        }
    }
    var preferredSourceFormat: VideoSourceFormat {
        didSet {
            defaults.set(Int(preferredSourceFormat.codec.rawValue), forKey: "sender.sourceCodec")
            defaults.set(preferredSourceFormat.resolution.rawValue, forKey: "sender.sourceHeight")
            defaults.set(Int(preferredSourceFormat.framesPerSecond), forKey: "sender.sourceFps")
            if senderStatus == .disconnected {
                activeBitrateBps = PicooSenderSession.initialBitrate(
                    forHeight: UInt32(preferredSourceFormat.resolution.rawValue)
                )
            }
        }
    }
    let camera: CameraCaptureModel
    let protocolName = PicooSenderSession.protocolName

    @ObservationIgnored private let session: PicooSenderSession?
    @ObservationIgnored private let mediaPipeline: SenderMediaPipeline?
    @ObservationIgnored private let defaults: UserDefaults
    @ObservationIgnored private var discoveryBrowser: PicooDiscoveryBrowser?
    @ObservationIgnored private var runtimeTask: Task<Void, Never>?
    @ObservationIgnored private var mediaTask: Task<Void, Never>?
    @ObservationIgnored private var mediaControlTask: Task<Void, Never>?
    @ObservationIgnored private var cameraLifecycleTask: Task<Void, Never>?
    @ObservationIgnored private var mediaControlGeneration: UInt64 = 0
    @ObservationIgnored private var pairingDeadline: ContinuousClock.Instant?
    @ObservationIgnored private var selectedReceiverID = ""
    @ObservationIgnored private var lastDiscoveryPollAt = ContinuousClock.now
    @ObservationIgnored private var stopResetTask: Task<Void, Never>?
    @ObservationIgnored private var waitingForSourceCapabilities = false
    @ObservationIgnored private var selectedInitialSourceFormat = false
    @ObservationIgnored private var isMediaSendEnabled = false
    @ObservationIgnored private var isSceneActive = true
    @ObservationIgnored private var lastHandledSessionError = ""
    @ObservationIgnored private let encoderApply = SenderEncoderApplyCoordinator()
    @ObservationIgnored private let discovery = SenderDiscoveryCoordinator()
    @ObservationIgnored private let wifiNetwork = PicooWifiNetworkMonitor()
    @ObservationIgnored private var discoveryInterfaceName: String?

    private static let autoConnectPreferenceKey = "sender.autoConnectEnabled"

    init(session: PicooSenderSession?) {
        let defaults = UserDefaults.standard
        self.defaults = defaults
        autoConnectEnabled = defaults.object(forKey: Self.autoConnectPreferenceKey) == nil
            ? true
            : defaults.bool(forKey: Self.autoConnectPreferenceKey)
        let storedPreferredSourceFormat = VideoSourceFormat(
            codec: UInt32(clamping: defaults.integer(forKey: "sender.sourceCodec")),
            height: UInt32(clamping: defaults.integer(forKey: "sender.sourceHeight")),
            framesPerSecond: UInt32(clamping: defaults.integer(forKey: "sender.sourceFps"))
        ) ?? .defaultFormat
        preferredSourceFormat = storedPreferredSourceFormat
        let snapshotBitrate = session?.snapshot.currentBitrateBps ?? 0
        let initialBitrate = snapshotBitrate > 0
            ? snapshotBitrate
            : PicooSenderSession.initialBitrate(
                forHeight: UInt32(storedPreferredSourceFormat.resolution.rawValue)
            )
        let initialEpoch = session?.snapshot.streamEpoch
            ?? PicooSenderSession.initialStreamEpoch
        activeBitrateBps = initialBitrate
        camera = CameraCaptureModel(
            initialBitrateBps: initialBitrate,
            initialStreamEpoch: initialEpoch
        )
        self.session = session
        mediaPipeline = session.map(SenderMediaPipeline.init)
        if session == nil {
            errorMessage = "Rust Core 初始化失败，请重新安装最新构建。"
            isDiscovering = false
        }
    }
    deinit {
        runtimeTask?.cancel()
        mediaTask?.cancel()
        mediaControlTask?.cancel()
        cameraLifecycleTask?.cancel()
        stopResetTask?.cancel()
    }

    func start() {
        guard runtimeTask == nil, session != nil else { return }
        trustedReceivers = session?.trustedReceivers() ?? []

        refreshDiscoveryBrowserForWifi()
        Task { [weak self] in await self?.camera.refreshSourceFormats() }

        runtimeTask = Task { [weak self] in
            var eventRevision: UInt64 = 0
            while !Task.isCancelled {
                guard let session = self?.senderSession else { return }
                let observedRevision = eventRevision
                eventRevision = await Task.detached {
                    session.waitForEvent(after: observedRevision, timeoutMs: 500)
                }.value
                guard !Task.isCancelled else { return }
                self?.tick()
            }
        }
        if let mediaPipeline {
            let signals = camera.encoderEventSignals
            mediaTask = Task { [weak self] in
                for await _ in signals {
                    guard !Task.isCancelled else { return }
                    guard let self else { return }
                    let events = self.camera.drainEncoderEvents()
                    for event in events {
                        guard event.streamEpoch == self.camera.streamEpoch,
                              event.encoderGeneration == self.camera.encoderGeneration
                        else {
                            continue
                        }
                        if case .failure = event {
                            self.suspendMediaSending()
                            self.encoderApply.handleFailure(event, host: self)
                            continue
                        }
                        guard self.isMediaSendEnabled || self.encoderApply.isPending,
                              self.matchesActiveMediaState
                        else {
                            continue
                        }
                        switch event {
                        case .queueOverflow:
                            await self.camera.requestKeyframe()
                        case let .accessUnit(accessUnit):
                            guard self.encoderApply.accepts(accessUnit) else { continue }
                            do {
                                let result = try await mediaPipeline.consume(event)
                                if result.keyframeRequested { await self.camera.requestKeyframe() }
                                if result.encoderAccepted {
                                    self.encoderApply.didCommit(accessUnit, host: self)
                                }
                            } catch {
                                self.errorMessage = error.localizedDescription
                            }
                        case .failure:
                            break
                        }
                    }
                }
            }
        }
    }

    func handleScenePhase(_ phase: ScenePhase) async {
        switch phase {
        case .active:
            isSceneActive = true
            if screen == .live {
                scheduleCameraActivation()
            }
        case .inactive, .background:
            isSceneActive = false
            suspendMediaSending()
            encoderApply.cancelPending(session: session)
            encoderApply.cancelRecovery()
            cameraLifecycleTask?.cancel()
            await camera.stop()
        @unknown default:
            break
        }
    }

    func refreshDiscovery() {
        lastDiscoveryPollAt = .now
        discovery.resetAutoConnect()
        pollDiscovery()
    }

    func removeTrustedReceiver(_ receiver: TrustedReceiverSummary) {
        do {
            try session?.removeTrustedReceiver(id: receiver.id)
            trustedReceivers = session?.trustedReceivers() ?? []
        } catch {
            errorMessage = "无法撤销对 \(receiver.name) 的信任。"
        }
    }

    func selectReceiver(_ receiver: ReceiverSummary) {
        selectedReceiverID = receiver.id
        receiverName = receiver.displayName
        receiverEndpoint = receiver.endpoint
        connect(to: receiver.endpoint)
    }

    func connectManualEndpoint() {
        guard let endpoint = ReceiverEndpoint(manualText: manualEndpointText) else {
            errorMessage = "请输入有效的局域网 IP:端口，例如 192.168.1.108:4433。"
            return
        }
        selectedReceiverID = "manual-\(endpoint.host)"
        receiverName = endpoint.displayText
        receiverEndpoint = endpoint
        if connect(to: endpoint) {
            isManualConnectPresented = false
        }
    }

    func confirmPairing() {
        guard pairingSecondsRemaining > 0,
              !pairingCode.isEmpty,
              let session
        else {
            return
        }

        let receiverID = session.connectedReceiverID.isEmpty
            ? selectedReceiverID
            : session.connectedReceiverID
        guard !receiverID.isEmpty else {
            errorMessage = "尚未收到电脑身份，请稍后再确认。"
            return
        }

        do {
            try session.confirmPairing(receiverID: receiverID)
            phoneConfirmedPairing = true
            pairingWaitOutcome = .pending
            screen = .waiting
            errorMessage = nil
        } catch {
            errorMessage = "发送配对确认失败，请重新连接。"
        }
    }

    func handleStopTap() {
        if stopArmed {
            disconnectImmediately()
            return
        }

        stopArmed = true
        stopResetTask?.cancel()
        stopResetTask = Task { [weak self] in
            try? await Task.sleep(for: .seconds(3))
            guard !Task.isCancelled else { return }
            self?.stopArmed = false
        }
    }

    func switchCamera() async {
        await applyCameraSwitch(unlessAlreadyAt: nil, failure: "无法切换摄像头。")
    }

    @discardableResult
    func connect(to endpoint: ReceiverEndpoint) -> Bool {
        guard let session else { return false }
        guard let wifiInterface = wifiNetwork.current else {
            errorMessage = "未连接 Wi-Fi，请连接电脑所在的 Wi-Fi 后重试。"
            return false
        }
        suspendMediaSending()
        selectedInitialSourceFormat = false
        phoneConfirmedPairing = false
        pairingWaitOutcome = .pending
        pairingCode = ""
        pairingDeadline = nil
        pairingSecondsRemaining = 60
        isConnecting = true
        errorMessage = nil
        lastHandledSessionError = ""

        do {
            activeBitrateBps = PicooSenderSession.initialBitrate(
                forHeight: UInt32(preferredSourceFormat.resolution.rawValue)
            )
            try session.setPreferredHeight(UInt32(preferredSourceFormat.resolution.rawValue))
            try session.connect(to: endpoint, wifiInterfaceIndex: wifiInterface.index)
            return true
        } catch PicooSenderSessionError.networkBindingFailed {
            isConnecting = false
            errorMessage = "当前 VPN 不允许局域网连接，请允许局域网访问或关闭 VPN 后重试。"
            return false
        } catch {
            isConnecting = false
            errorMessage = "无法连接 \(endpoint.displayText)，请确认电脑端已启动。"
            return false
        }
    }

    private func tick() {
        applySessionTick()
        encoderApply.reconcileCore(host: self)
        pollMediaControl()
    }

    var senderSession: PicooSenderSession? { session }

    var sceneIsActive: Bool { isSceneActive }

    func noteError(_ message: String?) {
        errorMessage = message
    }

    func applyDiscoveryPoll(
        receivers: [ReceiverSummary],
        trustedReceivers: [TrustedReceiverSummary],
        isDiscovering: Bool
    ) {
        self.trustedReceivers = trustedReceivers
        self.receivers = receivers
        self.isDiscovering = isDiscovering
    }

    func markDiscoveryUnavailable() {
        isDiscovering = false
        errorMessage = "局域网发现暂不可用，仍可输入 IP 地址直连。"
    }

    func commitAppliedEncoder(bitrateBps: UInt32, recoveryMessage: String?) {
        activeBitrateBps = bitrateBps
        isMediaSendEnabled = isSceneActive && matchesActiveMediaState
        if let recoveryMessage {
            errorMessage = "\(recoveryMessage)；已恢复上一视频配置。"
        }
    }

    func disableMediaSending() {
        isMediaSendEnabled = false
    }

    func resumeMediaAfterEncoderRollback(message: String) {
        isMediaSendEnabled = isSceneActive && matchesActiveMediaState
        errorMessage = message
    }

    func observeSession(
        _ session: PicooSenderSession,
        pumpError: String?
    ) {
        if let pumpError {
            errorMessage = pumpError
        }
        let snapshot = session.snapshot
        senderStatus = snapshot.status
        receiverSourceFormats = snapshot.receiverSourceFormats
        if waitingForSourceCapabilities, snapshot.receiverSourceFormats != nil, matchesActiveMediaState {
            waitingForSourceCapabilities = false
            scheduleCameraActivation()
        }
        isConnecting = senderStatus == .connecting
        if senderStatus == .reconnecting {
            reconnectAttempt = snapshot.reconnectAttempt
            reconnectDelayMs = snapshot.reconnectDelayMs
        } else {
            reconnectAttempt = 0
            reconnectDelayMs = 0
        }
        let observedName = session.connectedReceiverDisplayName
        if !observedName.isEmpty {
            receiverName = observedName
        }
        let observedPairingCode = session.pairingShortCode
        if observedPairingCode != pairingCode {
            pairingCode = observedPairingCode
            if !observedPairingCode.isEmpty {
                pairingDeadline = ContinuousClock.now.advanced(by: .seconds(60))
                pairingSecondsRemaining = 60
            }
        }
        updatePairingCountdown()
        let sessionError = session.lastSessionError
        if sessionError != lastHandledSessionError {
            lastHandledSessionError = sessionError
            if sessionError == "PAIRING_REJECTED" {
                pairingWaitOutcome = .rejected
                screen = .waiting
                errorMessage = nil
            }
        }
        if phoneConfirmedPairing, screen == .waiting,
           pairingSecondsRemaining == 0,
           senderStatus != .streaming,
           senderStatus != .reconnecting,
           senderStatus != .networkUnstable
        {
            pairingWaitOutcome = .expired
        }
        let resolved = pairingWaitOutcome == .pending
            ? SenderScreenResolver.resolve(
                status: senderStatus,
                pairingCode: pairingCode,
                phoneConfirmedPairing: phoneConfirmedPairing
            )
            : .waiting
        if resolved != screen {
            screen = resolved
            if resolved == .live, isSceneActive {
                scheduleCameraActivation()
            } else if screen == .devices {
                scheduleCameraStop()
            }
        }
    }

    private func updatePairingCountdown() {
        guard let pairingDeadline else { return }
        let remaining = ContinuousClock.now.duration(to: pairingDeadline)
        pairingSecondsRemaining = max(0, Int(remaining.components.seconds))
    }

    func discoveryPollIsDue(now: ContinuousClock.Instant = .now) -> Bool {
        guard lastDiscoveryPollAt.duration(to: now) >= .seconds(1) else {
            return false
        }
        lastDiscoveryPollAt = now
        return true
    }

    func pollDiscovery() {
        guard let session else { return }
        refreshDiscoveryBrowserForWifi()
        discovery.poll(session: session, browser: discoveryBrowser, host: self)
    }

    private func refreshDiscoveryBrowserForWifi() {
        let currentInterface = wifiNetwork.current
        guard currentInterface?.name != discoveryInterfaceName else { return }
        discoveryBrowser = nil
        discoveryInterfaceName = currentInterface?.name
        receivers = []
        guard let currentInterface else {
            isDiscovering = false
            return
        }
        do {
            discoveryBrowser = try PicooDiscoveryBrowser(interfaceName: currentInterface.name)
            isDiscovering = true
        } catch {
            isDiscovering = false
            discoveryInterfaceName = nil
            errorMessage = "局域网发现暂不可用，仍可输入 IP 地址直连。"
        }
    }

    private func activateCamera() async {
        guard let session, let mediaPipeline else { return }
        guard !Task.isCancelled else { return }
        if camera.state == .running {
            isMediaSendEnabled = matchesActiveMediaState
            return
        }
        suspendMediaSending()
        let initialSourceFormat: VideoSourceFormat
        let requestedSourceFormat: VideoSourceFormat?
        if selectedInitialSourceFormat {
            initialSourceFormat = camera.sourceFormat
            requestedSourceFormat = nil
        } else {
            initialSourceFormat = preferredSourceFormat
            requestedSourceFormat = initialSourceFormat
        }
        guard session.snapshot.receiverSourceFormats != nil else {
            waitingForSourceCapabilities = true
            return
        }
        await camera.refreshSourceFormats()
        guard !Task.isCancelled, availableSourceFormats?.contains(initialSourceFormat) == true else {
            errorMessage = "默认视频格式不可用，请选择可准备的完整格式。"
            return
        }
        if let requestedSourceFormat {
            activeBitrateBps = PicooSenderSession.initialBitrate(
                forHeight: UInt32(requestedSourceFormat.resolution.rawValue)
            )
        }
        let streamEpoch = encoderApply.beginLocal(
            session: session,
            sourceFormat: initialSourceFormat
        )
        guard streamEpoch > 0 else { return }
        await mediaPipeline.setMirrored(remoteMirrored)

        guard !Task.isCancelled else {
            _ = session.reportEncoderFailed(streamEpoch: streamEpoch, encoderGeneration: 0)
            return
        }
        let granted = await camera.start(
            sourceFormat: requestedSourceFormat,
            bitrateBps: activeBitrateBps,
            streamEpoch: streamEpoch
        )
        guard !Task.isCancelled else {
            _ = session.reportEncoderFailed(streamEpoch: streamEpoch, encoderGeneration: 0)
            await camera.stop()
            return
        }
        if granted {
            selectedInitialSourceFormat = true
            encoderApply.waitForApply(
                directive: nil,
                streamEpoch: streamEpoch,
                encoderGeneration: camera.encoderGeneration,
                sourceFormat: initialSourceFormat,
                bitrateBps: activeBitrateBps,
                session: session
            )
        }
        do {
            if granted {
                try session.clearCameraPermissionRequired()
            } else {
                _ = session.reportEncoderFailed(streamEpoch: streamEpoch, encoderGeneration: 0)
                try session.markCameraPermissionRequired()
            }
        } catch {
            errorMessage = "无法同步相机权限状态。"
        }
    }

    func disconnectImmediately() {
        suspendMediaSending()
        stopResetTask?.cancel()
        stopArmed = false
        phoneConfirmedPairing = false
        pairingWaitOutcome = .pending
        pairingCode = ""
        pairingDeadline = nil
        pairingSecondsRemaining = 60
        lastHandledSessionError = ""
        isConnecting = false
        errorMessage = nil
        mediaControlTask?.cancel()
        mediaControlTask = nil
        encoderApply.cancelRecovery()
        mediaControlGeneration &+= 1
        encoderApply.clearPending()
        try? session?.disconnect()
        screen = .devices
        selectedInitialSourceFormat = false
        waitingForSourceCapabilities = false
        scheduleCameraStop()
    }

    private func pollMediaControl() {
        guard isSceneActive, matchesActiveMediaState, let session else { return }

        let requestedBitrate = session.snapshot.currentBitrateBps
        if !encoderApply.isPending, requestedBitrate > 0, requestedBitrate != activeBitrateBps {
            activeBitrateBps = requestedBitrate
            Task { [weak self] in
                await self?.camera.updateBitrate(requestedBitrate)
            }
        }

        if (try? session.takeKeyframeRequest()) == true {
            Task { [weak self] in await self?.camera.requestKeyframe() }
        }

        guard mediaControlTask == nil, !encoderApply.isPending else { return }
        let cameraCommand = try? session.takeCameraCommand()
        let encoderDirective = cameraCommand == nil ? try? session.encoderDirective() : nil
        if let encoderDirective,
           !(session.snapshot.receiverSourceFormats ?? []).contains(where: {
               $0 == VideoSourceFormat(codec: encoderDirective.targetCodec, height: encoderDirective.targetHeight, framesPerSecond: encoderDirective.targetFps)
           })
        {
            encoderApply.rejectBeforeStart(encoderDirective, host: self)
            return
        }
        guard cameraCommand != nil || encoderDirective != nil else { return }

        mediaControlGeneration &+= 1
        let operation = mediaControlGeneration
        mediaControlTask = Task { [weak self] in
            guard let self else { return }
            guard !Task.isCancelled,
                  operation == self.mediaControlGeneration
            else {
                return
            }
            if let cameraCommand {
                await self.apply(cameraCommand)
            } else if let encoderDirective {
                if let source = VideoSourceFormat(codec: encoderDirective.targetCodec, height: encoderDirective.targetHeight, framesPerSecond: encoderDirective.targetFps) {
                    await self.applySourceFormat(source, directive: encoderDirective)
                } else {
                    self.encoderApply.rejectBeforeStart(encoderDirective, host: self)
                }
            }
            guard operation == self.mediaControlGeneration else { return }
            self.mediaControlTask = nil
        }
    }

    private func apply(_ command: SenderCameraCommand) async {
        switch command {
        case .switchFront:
            await applyCameraSwitch(unlessAlreadyAt: .front, failure: "电脑请求的前置摄像头不可用。")
        case .switchBack:
            await applyCameraSwitch(unlessAlreadyAt: .back, failure: "电脑请求的后置摄像头不可用。")
        case .switchCamera:
            await applyCameraSwitch(unlessAlreadyAt: nil, failure: "电脑请求切换的摄像头不可用。")
        case let .setResolution(width, height):
            guard let resolution = VideoResolution.supported(forRequestedHeight: height),
                  UInt32(resolution.width) == width else {
                errorMessage = "电脑请求的视频尺寸不受支持。"
                return
            }
            await applySourceFormat(VideoSourceFormat(codec: camera.sourceFormat.codec, resolution: resolution, framesPerSecond: camera.sourceFormat.framesPerSecond))
        case let .setMirror(mirrored):
            remoteMirrored = mirrored
            await mediaPipeline?.setMirrored(mirrored)
            await camera.requestKeyframe()
        }
    }

    private func applyCameraSwitch(
        unlessAlreadyAt position: CameraPosition?,
        failure: String
    ) async {
        if let position, camera.position == position { return }
        guard let session else { return }
        guard let targetSource = await preparedCameraSwitchSource() else {
            errorMessage = "目标镜头与接收端没有共同可用的视频格式。"
            return
        }
        suspendMediaSending()
        let epoch = encoderApply.beginLocal(
            session: session,
            sourceFormat: targetSource
        )
        guard epoch > 0 else { return }
        let switched = await camera.switchCamera(sourceFormat: targetSource, streamEpoch: epoch)
        guard !Task.isCancelled else {
            _ = session.reportEncoderFailed(streamEpoch: epoch, encoderGeneration: 0)
            return
        }
        if switched {
            encoderApply.waitForApply(
                directive: nil,
                streamEpoch: epoch,
                encoderGeneration: camera.encoderGeneration,
                sourceFormat: targetSource,
                bitrateBps: PicooSenderSession.initialBitrate(forHeight: UInt32(targetSource.resolution.rawValue)),
                session: session
            )
        } else {
            encoderApply.failBeforeStart(streamEpoch: epoch, message: failure, host: self)
        }
    }

    func applySourceFormat(
        _ source: VideoSourceFormat,
        directive: SenderEncoderDirective? = nil
    ) async {
        guard let session else { return }
        guard session.snapshot.receiverSourceFormats?.contains(source) == true else {
            errorMessage = "接收端不支持所选视频配置。"
            if let directive { encoderApply.rejectBeforeStart(directive, host: self) }
            return
        }
        await camera.refreshSourceFormats()
        guard !Task.isCancelled, availableSourceFormats?.contains(source) == true else {
            errorMessage = "当前镜头无法准备所选视频格式。"
            if let directive { encoderApply.rejectBeforeStart(directive, host: self) }
            return
        }
        suspendMediaSending()
        let targetBitrate = directive?.targetBitrateBps
            ?? PicooSenderSession.initialBitrate(
                forHeight: UInt32(source.resolution.rawValue)
            )
        let streamEpoch = directive?.streamEpoch
            ?? encoderApply.beginLocal(
                session: session,
                sourceFormat: source
            )
        guard streamEpoch > 0 else {
            errorMessage = "接收端要求先完成当前编码器调整。"
            return
        }
        let applied: Bool
        if camera.state == .running {
            applied = await camera.setSourceFormat(source, bitrateBps: targetBitrate, streamEpoch: streamEpoch)
        } else {
            applied = await camera.start(sourceFormat: source, bitrateBps: targetBitrate, streamEpoch: streamEpoch)
        }
        guard !Task.isCancelled else {
            _ = session.reportEncoderFailed(streamEpoch: streamEpoch, encoderGeneration: 0)
            return
        }
        guard applied else {
            encoderApply.failBeforeStart(
                streamEpoch: streamEpoch,
                message: "当前摄像头不支持 \(source.resolution.rawValue)P。",
                host: self
            )
            return
        }
        encoderApply.waitForApply(
            directive: directive,
            streamEpoch: streamEpoch,
            encoderGeneration: camera.encoderGeneration,
            sourceFormat: source,
            bitrateBps: targetBitrate,
            session: session
        )
    }

    func suspendMediaSending() {
        isMediaSendEnabled = false
        camera.discardEncoderEventsUntilKeyframe()
    }

    func scheduleCameraActivation() {
        guard isSceneActive else { return }
        cameraLifecycleTask?.cancel()
        cameraLifecycleTask = Task { [weak self] in
            await self?.activateCamera()
        }
    }

    func scheduleCameraStop() {
        suspendMediaSending()
        cameraLifecycleTask?.cancel()
        cameraLifecycleTask = Task { [weak self] in
            await self?.camera.stop()
        }
    }

    func scheduleReconnectRebuild() {
        guard isSceneActive, let session else { return }
        suspendMediaSending()
        let streamEpoch = encoderApply.beginLocal(
            session: session,
            sourceFormat: camera.sourceFormat
        )
        guard streamEpoch > 0 else { return }
        cameraLifecycleTask?.cancel()
        cameraLifecycleTask = Task { [weak self] in
            guard let self else { return }
            let rebuilt = await self.camera.rebuildAfterReconnect(streamEpoch: streamEpoch)
            guard !Task.isCancelled else {
                _ = session.reportEncoderFailed(
                    streamEpoch: streamEpoch,
                    encoderGeneration: 0
                )
                return
            }
            if !rebuilt {
                self.encoderApply.failBeforeStart(
                    streamEpoch: streamEpoch,
                    message: "网络恢复后无法重建视频编码器。",
                    host: self
                )
            } else {
                self.encoderApply.waitForApply(
                    directive: nil,
                    streamEpoch: streamEpoch,
                    encoderGeneration: self.camera.encoderGeneration,
                    sourceFormat: self.camera.sourceFormat,
                    bitrateBps: self.activeBitrateBps,
                    session: session
                )
            }
        }
    }
}
