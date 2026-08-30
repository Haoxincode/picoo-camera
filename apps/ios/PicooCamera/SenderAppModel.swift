import Observation
import SwiftUI

private struct PendingEncoderApply {
    let directive: SenderEncoderDirective?
    let streamEpoch: UInt32
    let encoderGeneration: UInt64
    let targetHeight: UInt32
    let targetBitrateBps: UInt32
    let deadline: ContinuousClock.Instant
    let recoveryMessage: String?
}

private struct CommittedEncoderState {
    let resolution: VideoResolution
    let position: CameraPosition
    let streamEpoch: UInt32
    let bitrateBps: UInt32
}

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

    var manualEndpointText = ""
    var isManualConnectPresented = false
    var isSettingsPresented = false

    let camera: CameraCaptureModel
    let protocolVersion = PicooSenderSession.protocolVersion

    @ObservationIgnored private let session: PicooSenderSession?
    @ObservationIgnored private let mediaPipeline: SenderMediaPipeline?
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
    @ObservationIgnored private var pendingEncoderApply: PendingEncoderApply?
    @ObservationIgnored private var committedEncoderState: CommittedEncoderState?
    @ObservationIgnored private var encoderRecoveryTask: Task<Void, Never>?

    init(session: PicooSenderSession?) {
        let initialBitrate = session?.snapshot.currentBitrateBps
            ?? PicooSenderSession.initialBitrate(forHeight: 1080)
        let initialEpoch = session?.snapshot.streamEpoch
            ?? PicooSenderSession.initialStreamEpoch
        activeBitrateBps = initialBitrate
        camera = CameraCaptureModel(
            initialBitrateBps: initialBitrate,
            initialStreamEpoch: initialEpoch
        )
        committedEncoderState = nil
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
        encoderRecoveryTask?.cancel()
        stopResetTask?.cancel()
    }

    func start() {
        guard runtimeTask == nil, session != nil else { return }

        do {
            discoveryBrowser = try PicooDiscoveryBrowser()
        } catch {
            isDiscovering = false
            errorMessage = "局域网发现暂不可用，仍可输入 IP 地址直连。"
        }

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
                        let wasApplyingEncoder = self.pendingEncoderApply != nil
                        let completedApply = self.completePendingEncoderApply(with: event)
                        if case let .failure(_, _, message) = event,
                           !wasApplyingEncoder
                        {
                            self.suspendMediaSending()
                            self.scheduleEncoderRecovery(after: message)
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
                        case .accessUnit where completedApply || self.pendingEncoderApply == nil,
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
            cancelPendingEncoderApply()
            encoderRecoveryTask?.cancel()
            encoderRecoveryTask = nil
            cameraLifecycleTask?.cancel()
            await camera.stop()
        @unknown default:
            break
        }
    }

    func refreshDiscovery() {
        lastDiscoveryTick = 0
        pollDiscovery()
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
        isManualConnectPresented = false
        connect(to: endpoint)
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
            screen = .waiting
            errorMessage = nil
        } catch {
            errorMessage = "发送配对确认失败，请重新连接。"
        }
    }

    func retryPairing() {
        guard let endpoint = receiverEndpoint else { return }
        disconnectImmediately()
        connect(to: endpoint)
    }

    func cancelConnection() {
        disconnectImmediately()
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
        guard let session else { return }
        suspendMediaSending()
        let streamEpoch = beginLocalEncoderReconfiguration(session)
        guard streamEpoch > 0 else { return }
        let switched = await camera.switchCamera(streamEpoch: streamEpoch)
        guard !Task.isCancelled else {
            try? session.cancelStreamReconfiguration(streamEpoch)
            return
        }
        if switched {
            waitForEncoderApply(
                directive: nil,
                streamEpoch: streamEpoch,
                height: UInt32(camera.resolution.rawValue),
                bitrateBps: activeBitrateBps
            )
        } else {
            try? session.cancelStreamReconfiguration(streamEpoch)
            scheduleEncoderRecovery(after: "无法切换摄像头。")
        }
    }

    func toggleResolution() async {
        let target: VideoResolution = switch camera.resolution {
        case .p1080: .p720
        case .p720: .p480
        case .p480: .p1080
        }
        await applyResolution(target)
    }

    var resolutionLabel: String {
        "\(camera.resolution.rawValue)P · 30"
    }

    var formattedPairingCode: String {
        guard pairingCode.count == 6 else { return pairingCode }
        let midpoint = pairingCode.index(pairingCode.startIndex, offsetBy: 3)
        return "\(pairingCode[..<midpoint]) \(pairingCode[midpoint...])"
    }

    private func connect(to endpoint: ReceiverEndpoint) {
        guard let session else { return }
        suspendMediaSending()
        selectedInitialResolution = false
        phoneConfirmedPairing = false
        pairingCode = ""
        pairingDeadline = nil
        pairingSecondsRemaining = 60
        isConnecting = true
        errorMessage = nil

        do {
            try session.setPreferredHeight(UInt32(VideoResolution.p1080.rawValue))
            try session.setStreamConfiguration(initialStreamConfiguration)
            try session.connect(to: endpoint)
        } catch {
            isConnecting = false
            errorMessage = "无法连接 \(endpoint.displayText)，请确认电脑端已启动。"
        }
    }

    private func tick() {
        guard let session else { return }
        do {
            try session.pump()
        } catch {
            errorMessage = "连接出现异常，正在等待重新连接。"
        }

        let previousSenderStatus = senderStatus
        let senderSnapshot = session.snapshot
        senderStatus = senderSnapshot.status
        isConnecting = senderStatus == .connecting
        if matchesActiveMediaState(previousSenderStatus),
           !matchesActiveMediaState
        {
            suspendMediaSending()
        }
        if isSceneActive,
           previousSenderStatus == .reconnecting,
           senderStatus == .streaming
        {
            scheduleReconnectRebuild()
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

        let resolved = SenderScreenResolver.resolve(
            status: senderStatus,
            pairingCode: pairingCode,
            phoneConfirmedPairing: phoneConfirmedPairing
        )
        if resolved != screen {
            screen = resolved
            if resolved == .live, isSceneActive {
                scheduleCameraActivation()
            } else if screen == .devices {
                scheduleCameraStop()
            }
        }

        lastDiscoveryTick += 1
        if lastDiscoveryTick >= 4 {
            lastDiscoveryTick = 0
            pollDiscovery()
        }
        pollMediaControl()
        expirePendingEncoderApplyIfNeeded()
    }

    private func pollDiscovery() {
        guard let discoveryBrowser, let session else { return }
        do {
            receivers = try discoveryBrowser.poll(
                trustedReceiverIDs: session.trustedReceiverIDs()
            )
            isDiscovering = receivers.isEmpty
        } catch {
            isDiscovering = false
            errorMessage = "局域网发现暂不可用，仍可输入 IP 地址直连。"
        }
    }

    private func updatePairingCountdown() {
        guard let pairingDeadline else { return }
        let remaining = ContinuousClock.now.duration(to: pairingDeadline)
        let seconds = max(0, Int(remaining.components.seconds))
        pairingSecondsRemaining = seconds
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
            initialResolution = receiverMaxHeight > 0 && receiverMaxHeight < 1080
                ? VideoResolution.supported(forRequestedHeight: receiverMaxHeight)
                : .p1080
            requestedResolution = initialResolution
        }
        if let requestedResolution {
            activeBitrateBps = PicooSenderSession.initialBitrate(
                forHeight: UInt32(requestedResolution.rawValue)
            )
        }
        let streamEpoch = selectedInitialResolution
            ? beginLocalEncoderReconfiguration(session)
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
            waitForEncoderApply(
                directive: nil,
                streamEpoch: streamEpoch,
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

    private func disconnectImmediately() {
        suspendMediaSending()
        stopResetTask?.cancel()
        stopArmed = false
        phoneConfirmedPairing = false
        pairingCode = ""
        pairingDeadline = nil
        pairingSecondsRemaining = 60
        isConnecting = false
        errorMessage = nil
        mediaControlTask?.cancel()
        mediaControlTask = nil
        encoderRecoveryTask?.cancel()
        encoderRecoveryTask = nil
        mediaControlGeneration &+= 1
        pendingEncoderApply = nil
        try? session?.disconnect()
        screen = .devices
        selectedInitialResolution = false
        scheduleCameraStop()
    }

    private var initialStreamConfiguration: SenderStreamConfiguration {
        SenderStreamConfiguration(
            width: UInt32(camera.resolution.width),
            height: UInt32(camera.resolution.height),
            framesPerSecond: 30,
            bitrateBps: activeBitrateBps,
            streamEpoch: camera.streamEpoch,
            mirrored: remoteMirrored,
            rotation: 0,
            sequenceParameterSet: Data(),
            pictureParameterSet: Data()
        )
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

        guard mediaControlTask == nil, pendingEncoderApply == nil else { return }
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
            guard camera.position != .front, let session else { return }
            suspendMediaSending()
            let epoch = beginLocalEncoderReconfiguration(session)
            guard epoch > 0 else { return }
            let switched = await camera.switchCamera(streamEpoch: epoch)
            guard !Task.isCancelled else {
                try? session.cancelStreamReconfiguration(epoch)
                return
            }
            if switched {
                waitForEncoderApply(
                    directive: nil,
                    streamEpoch: epoch,
                    height: UInt32(camera.resolution.rawValue),
                    bitrateBps: activeBitrateBps
                )
            } else {
                try? session.cancelStreamReconfiguration(epoch)
                scheduleEncoderRecovery(after: "电脑请求的前置摄像头不可用。")
            }
        case .switchBack:
            guard camera.position != .back, let session else { return }
            suspendMediaSending()
            let epoch = beginLocalEncoderReconfiguration(session)
            guard epoch > 0 else { return }
            let switched = await camera.switchCamera(streamEpoch: epoch)
            guard !Task.isCancelled else {
                try? session.cancelStreamReconfiguration(epoch)
                return
            }
            if switched {
                waitForEncoderApply(
                    directive: nil,
                    streamEpoch: epoch,
                    height: UInt32(camera.resolution.rawValue),
                    bitrateBps: activeBitrateBps
                )
            } else {
                try? session.cancelStreamReconfiguration(epoch)
                scheduleEncoderRecovery(after: "电脑请求的后置摄像头不可用。")
            }
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

    private func applyResolution(
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
            ?? beginLocalEncoderReconfiguration(session)
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
            scheduleEncoderRecovery(
                after: "当前摄像头不支持 \(supportedResolution.rawValue)P。"
            )
            return
        }
        waitForEncoderApply(
            directive: directive,
            streamEpoch: streamEpoch,
            height: UInt32(supportedResolution.rawValue),
            bitrateBps: targetBitrate
        )
    }

    private func waitForEncoderApply(
        directive: SenderEncoderDirective?,
        streamEpoch: UInt32,
        height: UInt32,
        bitrateBps: UInt32
    ) {
        pendingEncoderApply = PendingEncoderApply(
            directive: directive,
            streamEpoch: streamEpoch,
            encoderGeneration: camera.encoderGeneration,
            targetHeight: height,
            targetBitrateBps: bitrateBps,
            deadline: ContinuousClock.now.advanced(by: .seconds(3)),
            recoveryMessage: nil
        )
    }

    private func beginLocalEncoderReconfiguration(
        _ session: PicooSenderSession
    ) -> UInt32 {
        encoderRecoveryTask?.cancel()
        encoderRecoveryTask = nil
        cancelPendingEncoderApply()
        if let queuedDirective = try? session.encoderDirective() {
            try? session.rejectEncoderDirective(queuedDirective.id)
        }
        return session.beginStreamReconfiguration()
    }

    private func completePendingEncoderApply(with event: VideoEncoderEvent) -> Bool {
        guard let pending = pendingEncoderApply, let session else { return false }
        guard event.streamEpoch == pending.streamEpoch,
              event.encoderGeneration == pending.encoderGeneration
        else {
            return false
        }
        switch event {
        case let .accessUnit(accessUnit):
            guard accessUnit.isKeyframe,
                  accessUnit.streamEpoch == pending.streamEpoch,
                  accessUnit.height == pending.targetHeight
            else {
                return false
            }
            do {
                if let directive = pending.directive {
                    try session.acknowledgeEncoderDirective(
                        directive.id,
                        actualHeight: accessUnit.height
                    )
                } else {
                    try session.reportEncoderHeight(
                        accessUnit.height,
                        streamEpoch: accessUnit.streamEpoch
                    )
                }
                activeBitrateBps = pending.targetBitrateBps
                committedEncoderState = CommittedEncoderState(
                    resolution: VideoResolution.supported(
                        forRequestedHeight: accessUnit.height
                    ),
                    position: camera.position,
                    streamEpoch: accessUnit.streamEpoch,
                    bitrateBps: pending.targetBitrateBps
                )
                pendingEncoderApply = nil
                isMediaSendEnabled = isSceneActive && matchesActiveMediaState
                if let recoveryMessage = pending.recoveryMessage {
                    errorMessage = "\(recoveryMessage)；已恢复上一视频配置。"
                }
                return true
            } catch {
                failPendingEncoderApply("无法同步视频分辨率。")
                return false
            }
        case let .failure(_, _, message):
            failPendingEncoderApply(message)
            return false
        case .queueOverflow:
            return false
        }
    }

    private func expirePendingEncoderApplyIfNeeded() {
        guard let pending = pendingEncoderApply,
              ContinuousClock.now >= pending.deadline
        else {
            return
        }
        failPendingEncoderApply("编码器未在 3 秒内输出目标关键帧。")
    }

    private func failPendingEncoderApply(_ message: String) {
        guard let pending = pendingEncoderApply, let session else { return }
        if let directive = pending.directive {
            try? session.rejectEncoderDirective(directive.id)
        } else {
            try? session.cancelStreamReconfiguration(pending.streamEpoch)
        }
        pendingEncoderApply = nil
        isMediaSendEnabled = false
        if pending.recoveryMessage != nil {
            disconnectImmediately()
            errorMessage = "\(message)；恢复上一视频配置失败，已断开连接。"
            return
        }
        scheduleEncoderRecovery(after: message)
    }

    private func cancelPendingEncoderApply() {
        guard let pending = pendingEncoderApply, let session else { return }
        if let directive = pending.directive {
            try? session.rejectEncoderDirective(directive.id)
        } else {
            try? session.cancelStreamReconfiguration(pending.streamEpoch)
        }
        pendingEncoderApply = nil
    }

    private func scheduleEncoderRecovery(after message: String) {
        encoderRecoveryTask?.cancel()
        guard let committed = committedEncoderState else {
            disconnectImmediately()
            errorMessage = "\(message)；尚无可恢复的视频配置，已断开连接。"
            return
        }
        encoderRecoveryTask = Task { [weak self] in
            guard let self else { return }
            let restored = await self.camera.restoreCommittedConfiguration(
                resolution: committed.resolution,
                position: committed.position,
                bitrateBps: committed.bitrateBps,
                streamEpoch: committed.streamEpoch
            )
            guard !Task.isCancelled else { return }
            self.encoderRecoveryTask = nil
            guard restored else {
                self.disconnectImmediately()
                self.errorMessage = "\(message)；恢复上一视频配置失败，已断开连接。"
                return
            }
            self.pendingEncoderApply = PendingEncoderApply(
                directive: nil,
                streamEpoch: committed.streamEpoch,
                encoderGeneration: self.camera.encoderGeneration,
                targetHeight: UInt32(committed.resolution.rawValue),
                targetBitrateBps: committed.bitrateBps,
                deadline: ContinuousClock.now.advanced(by: .seconds(3)),
                recoveryMessage: message
            )
            await self.camera.requestKeyframe()
        }
    }

    private var matchesActiveMediaState: Bool {
        matchesActiveMediaState(senderStatus)
    }

    private func matchesActiveMediaState(_ status: PicooSenderStatus) -> Bool {
        status == .streaming || status == .networkUnstable
    }

    private func suspendMediaSending() {
        isMediaSendEnabled = false
        camera.discardEncoderEventsUntilKeyframe()
    }

    private func scheduleCameraActivation() {
        guard isSceneActive else { return }
        cameraLifecycleTask?.cancel()
        cameraLifecycleTask = Task { [weak self] in
            await self?.activateCamera()
        }
    }

    private func scheduleCameraStop() {
        suspendMediaSending()
        cameraLifecycleTask?.cancel()
        cameraLifecycleTask = Task { [weak self] in
            await self?.camera.stop()
        }
    }

    private func scheduleReconnectRebuild() {
        guard isSceneActive, let session else { return }
        suspendMediaSending()
        let streamEpoch = beginLocalEncoderReconfiguration(session)
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
                self.scheduleEncoderRecovery(after: "网络恢复后无法重建视频编码器。")
            } else {
                self.waitForEncoderApply(
                    directive: nil,
                    streamEpoch: streamEpoch,
                    height: UInt32(self.camera.resolution.rawValue),
                    bitrateBps: self.activeBitrateBps
                )
            }
        }
    }
}
