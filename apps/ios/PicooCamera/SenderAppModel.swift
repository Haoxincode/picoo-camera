import Observation
import SwiftUI

// REQ-PICOO-UI-010: SwiftUI observes snapshots and sends actions; Rust owns
// the protocol/session state machine, AVFoundation owns camera lifecycle.
@MainActor
@Observable
final class SenderAppModel {
    private(set) var screen: SenderScreen = .devices
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
    var preferredResolution: VideoResolution {
        didSet {
            defaults.set(preferredResolution.rawValue, forKey: Self.resolutionPreferenceKey)
            if senderStatus == .disconnected {
                activeBitrateBps = PicooSenderSession.initialBitrate(
                    forHeight: UInt32(preferredResolution.rawValue)
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
    @ObservationIgnored private var lastDiscoveryTick = 0
    @ObservationIgnored private var stopResetTask: Task<Void, Never>?
    @ObservationIgnored private var selectedInitialResolution = false
    @ObservationIgnored private var isMediaSendEnabled = false
    @ObservationIgnored private var isSceneActive = true
    @ObservationIgnored private var lastHandledSessionError = ""
    @ObservationIgnored private let encoderApply = SenderEncoderApplyCoordinator()
    @ObservationIgnored private let discovery = SenderDiscoveryCoordinator()
    @ObservationIgnored private let wifiNetwork = PicooWifiNetworkMonitor()
    @ObservationIgnored private var discoveryInterfaceName: String?

    private static let autoConnectPreferenceKey = "sender.autoConnectEnabled"
    private static let resolutionPreferenceKey = "sender.preferredResolution"

    init(session: PicooSenderSession?) {
        let defaults = UserDefaults.standard
        self.defaults = defaults
        autoConnectEnabled = defaults.object(forKey: Self.autoConnectPreferenceKey) == nil
            ? true
            : defaults.bool(forKey: Self.autoConnectPreferenceKey)
        let storedPreferredResolution = VideoResolution(
            rawValue: defaults.integer(forKey: Self.resolutionPreferenceKey)
        ) ?? .p1080
        preferredResolution = storedPreferredResolution
        let snapshotBitrate = session?.snapshot.currentBitrateBps ?? 0
        let initialBitrate = snapshotBitrate > 0
            ? snapshotBitrate
            : PicooSenderSession.initialBitrate(
                forHeight: UInt32(storedPreferredResolution.rawValue)
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

        runtimeTask = Task { [weak self] in
            while !Task.isCancelled {
                self?.tick()
                try? await Task.sleep(for: .milliseconds(250))
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
                        let wasApplyingEncoder = self.encoderApply.isPending
                        let completedApply = self.encoderApply.complete(with: event, host: self)
                        if case let .failure(_, _, message) = event,
                           !wasApplyingEncoder
                        {
                            self.suspendMediaSending()
                            self.encoderApply.scheduleRecovery(after: message, host: self)
                            continue
                        }
                        guard self.isMediaSendEnabled,
                              self.matchesActiveMediaState
                        else {
                            continue
                        }
                        switch event {
                        case .queueOverflow:
                            await self.camera.requestKeyframe()
                        case .accessUnit where completedApply || !self.encoderApply.isPending,
                             .failure:
                            do {
                                try await mediaPipeline.consume(event)
                            } catch {
                                self.errorMessage = error.localizedDescription
                            }
                        case .accessUnit:
                            continue
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
        lastDiscoveryTick = 0
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
        selectedInitialResolution = false
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
                forHeight: UInt32(preferredResolution.rawValue)
            )
            try session.setPreferredHeight(UInt32(preferredResolution.rawValue))
            try session.setStreamConfiguration(initialStreamConfiguration)
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
        pollMediaControl()
        encoderApply.expireIfNeeded(host: self)
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

    func observeSession(
        _ session: PicooSenderSession,
        pumpError: String?
    ) {
        if let pumpError {
            errorMessage = pumpError
        }
        let snapshot = session.snapshot
        senderStatus = snapshot.status
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

    func bumpDiscoveryTick() -> Bool {
        lastDiscoveryTick += 1
        if lastDiscoveryTick >= 4 {
            lastDiscoveryTick = 0
            return true
        }
        return false
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
        let initialResolution: VideoResolution
        let requestedResolution: VideoResolution?
        if selectedInitialResolution {
            initialResolution = camera.resolution
            requestedResolution = nil
        } else {
            let receiverMaxHeight = session.snapshot.receiverMaxHeight
            let preferredHeight = UInt32(preferredResolution.rawValue)
            let requestedHeight = receiverMaxHeight > 0
                ? min(preferredHeight, receiverMaxHeight)
                : preferredHeight
            initialResolution = VideoResolution.supported(forRequestedHeight: requestedHeight)
            requestedResolution = initialResolution
        }
        if let requestedResolution {
            activeBitrateBps = PicooSenderSession.initialBitrate(
                forHeight: UInt32(requestedResolution.rawValue)
            )
        }
        let streamEpoch = selectedInitialResolution
            ? encoderApply.beginLocal(
                session: session,
                targetHeight: UInt32(initialResolution.rawValue)
            )
            : session.snapshot.streamEpoch
        guard streamEpoch > 0 else { return }
        if !selectedInitialResolution {
            do {
                try await mediaPipeline.prime(
                    resolution: initialResolution,
                    bitrateBps: activeBitrateBps,
                    streamEpoch: streamEpoch,
                    mirrored: remoteMirrored
                )
            } catch {
                errorMessage = "无法准备视频传输参数。"
                return
            }
        }

        guard !Task.isCancelled else {
            if selectedInitialResolution {
                try? session.cancelStreamReconfiguration(streamEpoch)
            }
            return
        }
        let granted = await camera.start(
            resolution: requestedResolution,
            bitrateBps: activeBitrateBps,
            streamEpoch: streamEpoch
        )
        guard !Task.isCancelled else {
            if selectedInitialResolution {
                try? session.cancelStreamReconfiguration(streamEpoch)
            }
            await camera.stop()
            return
        }
        if granted {
            selectedInitialResolution = true
            encoderApply.waitForApply(
                directive: nil,
                streamEpoch: streamEpoch,
                encoderGeneration: camera.encoderGeneration,
                height: UInt32(initialResolution.rawValue),
                bitrateBps: activeBitrateBps
            )
        }
        do {
            if granted {
                try session.clearCameraPermissionRequired()
            } else {
                if streamEpoch != session.snapshot.streamEpoch {
                    try? session.cancelStreamReconfiguration(streamEpoch)
                }
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
        selectedInitialResolution = false
        scheduleCameraStop()
    }

    private func pollMediaControl() {
        guard isSceneActive, matchesActiveMediaState, let session else { return }

        let requestedBitrate = session.snapshot.currentBitrateBps
        if requestedBitrate > 0, requestedBitrate != activeBitrateBps {
            activeBitrateBps = requestedBitrate
            Task { [weak self] in
                await self?.camera.updateBitrate(requestedBitrate)
            }
        }

        if (try? session.takeKeyframeRequest()) == true {
            Task { [weak self] in await self?.camera.requestKeyframe() }
        }

        guard mediaControlTask == nil, !encoderApply.isPending else { return }
        let receiverMaxHeight = session.snapshot.receiverMaxHeight
        if camera.state == .running,
           receiverMaxHeight > 0,
           UInt32(camera.resolution.rawValue) > receiverMaxHeight
        {
            mediaControlTask = Task { [weak self] in
                guard let self else { return }
                await self.applyResolution(
                    VideoResolution.supported(forRequestedHeight: receiverMaxHeight)
                )
                self.mediaControlTask = nil
            }
            return
        }
        let cameraCommand = try? session.takeCameraCommand()
        let encoderDirective = cameraCommand == nil ? try? session.encoderDirective() : nil
        if let encoderDirective,
           receiverMaxHeight > 0,
           encoderDirective.targetHeight > receiverMaxHeight
        {
            try? session.rejectEncoderDirective(encoderDirective.id)
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
                await self.applyResolution(
                    VideoResolution.supported(
                        forRequestedHeight: encoderDirective.targetHeight
                    ),
                    directive: encoderDirective
                )
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
        case let .setResolution(_, height):
            await applyResolution(VideoResolution.supported(forRequestedHeight: height))
        case let .setMirror(mirrored):
            remoteMirrored = mirrored
            do {
                try await mediaPipeline?.setMirrored(mirrored)
            } catch {
                errorMessage = "无法更新远端镜像设置。"
            }
        }
    }

    private func applyCameraSwitch(
        unlessAlreadyAt position: CameraPosition?,
        failure: String
    ) async {
        if let position, camera.position == position { return }
        guard let session else { return }
        suspendMediaSending()
        let epoch = encoderApply.beginLocal(
            session: session,
            targetHeight: UInt32(camera.resolution.rawValue)
        )
        guard epoch > 0 else { return }
        let switched = await camera.switchCamera(streamEpoch: epoch)
        guard !Task.isCancelled else {
            try? session.cancelStreamReconfiguration(epoch)
            return
        }
        if switched {
            encoderApply.waitForApply(
                directive: nil,
                streamEpoch: epoch,
                encoderGeneration: camera.encoderGeneration,
                height: UInt32(camera.resolution.rawValue),
                bitrateBps: activeBitrateBps
            )
        } else {
            try? session.cancelStreamReconfiguration(epoch)
            encoderApply.scheduleRecovery(after: failure, host: self)
        }
    }

    func applyResolution(
        _ resolution: VideoResolution,
        directive: SenderEncoderDirective? = nil
    ) async {
        guard let session else { return }
        let supportedResolution = resolution.clamped(
            toMaximumHeight: session.snapshot.receiverMaxHeight
        )
        if let directive,
           UInt32(supportedResolution.rawValue) != directive.targetHeight
        {
            try? session.rejectEncoderDirective(directive.id)
            errorMessage = "接收端能力不支持电脑请求的 \(directive.targetHeight)P。"
            return
        }
        suspendMediaSending()
        let targetBitrate = directive?.targetBitrateBps
            ?? PicooSenderSession.initialBitrate(
                forHeight: UInt32(supportedResolution.rawValue)
            )
        let streamEpoch = directive?.streamEpoch
            ?? encoderApply.beginLocal(
                session: session,
                targetHeight: UInt32(supportedResolution.rawValue)
            )
        guard streamEpoch > 0 else {
            errorMessage = "接收端要求先完成当前编码器调整。"
            return
        }
        let applied = await camera.setResolution(
            supportedResolution,
            bitrateBps: targetBitrate,
            streamEpoch: streamEpoch
        )
        guard !Task.isCancelled else {
            if let directive {
                try? session.rejectEncoderDirective(directive.id)
            } else {
                try? session.cancelStreamReconfiguration(streamEpoch)
            }
            return
        }
        guard applied else {
            if let directive {
                try? session.rejectEncoderDirective(directive.id)
            } else {
                try? session.cancelStreamReconfiguration(streamEpoch)
            }
            encoderApply.scheduleRecovery(
                after: "当前摄像头不支持 \(supportedResolution.rawValue)P。",
                host: self
            )
            return
        }
        encoderApply.waitForApply(
            directive: directive,
            streamEpoch: streamEpoch,
            encoderGeneration: camera.encoderGeneration,
            height: UInt32(supportedResolution.rawValue),
            bitrateBps: targetBitrate
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
            targetHeight: UInt32(camera.resolution.rawValue)
        )
        guard streamEpoch > 0 else { return }
        cameraLifecycleTask?.cancel()
        cameraLifecycleTask = Task { [weak self] in
            guard let self else { return }
            let rebuilt = await self.camera.rebuildAfterReconnect(streamEpoch: streamEpoch)
            guard !Task.isCancelled else {
                try? session.cancelStreamReconfiguration(streamEpoch)
                return
            }
            if !rebuilt {
                try? session.cancelStreamReconfiguration(streamEpoch)
                self.encoderApply.scheduleRecovery(after: "网络恢复后无法重建视频编码器。", host: self)
            } else {
                self.encoderApply.waitForApply(
                    directive: nil,
                    streamEpoch: streamEpoch,
                    encoderGeneration: self.camera.encoderGeneration,
                    height: UInt32(self.camera.resolution.rawValue),
                    bitrateBps: self.activeBitrateBps
                )
            }
        }
    }
}
