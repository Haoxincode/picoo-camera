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

    var manualEndpointText = ""
    var isManualConnectPresented = false
    var isSettingsPresented = false

    let camera = CameraCaptureModel()
    let protocolVersion = PicooSenderSession.protocolVersion

    @ObservationIgnored private let session: PicooSenderSession?
    @ObservationIgnored private var discoveryBrowser: PicooDiscoveryBrowser?
    @ObservationIgnored private var runtimeTask: Task<Void, Never>?
    @ObservationIgnored private var pairingDeadline: ContinuousClock.Instant?
    @ObservationIgnored private var selectedReceiverID = ""
    @ObservationIgnored private var lastDiscoveryTick = 0
    @ObservationIgnored private var stopResetTask: Task<Void, Never>?

    init(session: PicooSenderSession?) {
        self.session = session
        if session == nil {
            errorMessage = "Rust Core 初始化失败，请重新安装最新构建。"
            isDiscovering = false
        }
    }

    deinit {
        runtimeTask?.cancel()
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
    }

    func handleScenePhase(_ phase: ScenePhase) async {
        switch phase {
        case .active:
            if screen == .live {
                await activateCamera()
            }
        case .inactive, .background:
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
        await camera.switchCamera()
    }

    var formattedPairingCode: String {
        guard pairingCode.count == 6 else { return pairingCode }
        let midpoint = pairingCode.index(pairingCode.startIndex, offsetBy: 3)
        return "\(pairingCode[..<midpoint]) \(pairingCode[midpoint...])"
    }

    private func connect(to endpoint: ReceiverEndpoint) {
        guard let session else { return }
        phoneConfirmedPairing = false
        pairingCode = ""
        pairingDeadline = nil
        pairingSecondsRemaining = 60
        isConnecting = true
        errorMessage = nil

        do {
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

        senderStatus = session.status
        isConnecting = senderStatus == .connecting

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
            if resolved == .live {
                Task { [weak self] in await self?.activateCamera() }
            } else if screen == .devices {
                Task { [weak self] in await self?.camera.stop() }
            }
        }

        lastDiscoveryTick += 1
        if lastDiscoveryTick >= 4 {
            lastDiscoveryTick = 0
            pollDiscovery()
        }
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
        guard let session else { return }
        let granted = await camera.start()
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
        stopResetTask?.cancel()
        stopArmed = false
        phoneConfirmedPairing = false
        pairingCode = ""
        pairingDeadline = nil
        pairingSecondsRemaining = 60
        isConnecting = false
        errorMessage = nil
        try? session?.disconnect()
        screen = .devices
        Task { [weak self] in await self?.camera.stop() }
    }
}
