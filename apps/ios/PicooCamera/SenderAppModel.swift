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
    private(set) var activeBitrateBps = VideoBitrate.initial(for: .p1080)

    var manualEndpointText = ""
    var isManualConnectPresented = false
    var isSettingsPresented = false

    let camera = CameraCaptureModel()
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

    init(session: PicooSenderSession?) {
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
                    guard self.isMediaSendEnabled,
                          self.matchesActiveMediaState
                    else {
                        continue
                    }
                    for event in events {
                        switch event {
                        case .queueOverflow:
                            await self.camera.requestKeyframe()
                        case .accessUnit, .failure:
                            do {
                                try await mediaPipeline.consume(event)
                            } catch {
                                self.errorMessage = error.localizedDescription
                            }
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
        suspendMediaSending()
        if await camera.switchCamera() {
            await resumeMediaSending()
        } else {
            errorMessage = "无法切换摄像头，已继续使用当前镜头。"
            await resumeMediaSending()
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
        senderStatus = session.status
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
        suspendMediaSending()
        camera.prepareForStreamingStart()
        let initialResolution: VideoResolution
        let requestedResolution: VideoResolution?
        if selectedInitialResolution {
            initialResolution = camera.resolution
            requestedResolution = nil
        } else {
            let receiverMaxHeight = session.receiverMaxHeight
            initialResolution = receiverMaxHeight > 0 && receiverMaxHeight < 1080
                ? VideoResolution.supported(forRequestedHeight: receiverMaxHeight)
                : .p1080
            requestedResolution = initialResolution
        }
        if let requestedResolution {
            activeBitrateBps = VideoBitrate.initial(for: requestedResolution)
        }
        do {
            try session.syncEncodeHeight(UInt32(initialResolution.rawValue))
            try await mediaPipeline.prime(
                resolution: initialResolution,
                bitrateBps: activeBitrateBps,
                streamEpoch: camera.streamEpoch,
                mirrored: remoteMirrored
            )
        } catch {
            errorMessage = "无法准备视频传输参数。"
            return
        }

        guard !Task.isCancelled else { return }
        let granted = await camera.start(resolution: requestedResolution)
        guard !Task.isCancelled else { return }
        if granted {
            selectedInitialResolution = true
            await resumeMediaSending()
        }
        do {
            if granted {
                try session.clearCameraPermissionRequired()
            } else {
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
        mediaControlGeneration &+= 1
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

        let requestedBitrate = session.currentBitrateBps
        let clampedBitrate = VideoBitrate.clamp(
            requestedBitrate,
            for: camera.resolution
        )
        if requestedBitrate > 0, clampedBitrate != activeBitrateBps {
            activeBitrateBps = clampedBitrate
            Task { [weak self] in
                await self?.camera.updateBitrate(clampedBitrate)
            }
        }

        if (try? session.takeKeyframeRequest()) == true {
            Task { [weak self] in await self?.camera.requestKeyframe() }
        }

        guard mediaControlTask == nil else { return }
        let cameraCommand = try? session.takeCameraCommand()
        let downshift = cameraCommand == nil
            && (try? session.takeResolutionDownshift()) == true
        let upshift = cameraCommand == nil
            && !downshift
            && (try? session.takeResolutionUpshift()) == true
        guard cameraCommand != nil || downshift || upshift else { return }

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
            } else if downshift {
                let next: VideoResolution? = switch self.camera.resolution {
                case .p1080: .p720
                case .p720: .p480
                case .p480: nil
                }
                if let next { await self.applyResolution(next) }
            } else if upshift {
                let next: VideoResolution? = switch self.camera.resolution {
                case .p480: .p720
                case .p720: .p1080
                case .p1080: nil
                }
                let receiverMaxHeight = session.receiverMaxHeight
                if let next,
                   receiverMaxHeight == 0
                    || UInt32(next.rawValue) <= receiverMaxHeight
                {
                    await self.applyResolution(next)
                }
            }
            guard operation == self.mediaControlGeneration else { return }
            self.mediaControlTask = nil
        }
    }

    private func apply(_ command: SenderCameraCommand) async {
        switch command {
        case .switchFront:
            if camera.position != .front, !(await camera.switchCamera()) {
                errorMessage = "电脑请求的前置摄像头不可用。"
            }
        case .switchBack:
            if camera.position != .back, !(await camera.switchCamera()) {
                errorMessage = "电脑请求的后置摄像头不可用。"
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

    private func applyResolution(_ resolution: VideoResolution) async {
        guard let session else { return }
        suspendMediaSending()
        let supportedResolution = resolution.clamped(
            toMaximumHeight: session.receiverMaxHeight
        )
        guard await camera.setResolution(supportedResolution) else {
            errorMessage = "当前摄像头不支持 \(supportedResolution.rawValue)P。"
            await resumeMediaSending()
            return
        }
        activeBitrateBps = VideoBitrate.initial(for: supportedResolution)
        do {
            try session.syncEncodeHeight(UInt32(supportedResolution.rawValue))
        } catch {
            errorMessage = "无法同步视频分辨率。"
        }
        await resumeMediaSending()
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

    private func resumeMediaSending() async {
        camera.discardEncoderEventsUntilKeyframe()
        guard !Task.isCancelled, isSceneActive, matchesActiveMediaState else { return }
        isMediaSendEnabled = true
        await camera.requestKeyframe()
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
        guard isSceneActive else { return }
        suspendMediaSending()
        cameraLifecycleTask?.cancel()
        cameraLifecycleTask = Task { [weak self] in
            guard let self else { return }
            let rebuilt = await self.camera.rebuildAfterReconnect()
            guard !Task.isCancelled else { return }
            if !rebuilt {
                self.errorMessage = "网络恢复后无法重建视频编码器。"
            } else {
                await self.resumeMediaSending()
            }
        }
    }
}
