import Foundation

extension SenderAppModel {
    var preferredResolutionLabel: String {
        "\(preferredResolution.rawValue)P · 30 FPS"
    }

    func setPreferredResolution(_ resolution: VideoResolution) {
        preferredResolution = resolution
    }

    var resolutionLabel: String {
        "\(camera.resolution.rawValue)P · 30"
    }

    var formattedPairingCode: String {
        guard pairingCode.count == 6 else { return pairingCode }
        let midpoint = pairingCode.index(pairingCode.startIndex, offsetBy: 3)
        return "\(pairingCode[..<midpoint]) \(pairingCode[midpoint...])"
    }

    var matchesActiveMediaState: Bool {
        matchesActiveMediaState(senderStatus)
    }

    func matchesActiveMediaState(_ status: PicooSenderStatus) -> Bool {
        status == .streaming || status == .networkUnstable
    }

    func retryPairing() {
        guard let endpoint = receiverEndpoint else { return }
        disconnectImmediately()
        connect(to: endpoint)
    }

    func cancelConnection() {
        disconnectImmediately()
    }

    var initialStreamConfiguration: SenderStreamConfiguration {
        SenderStreamConfiguration(
            width: UInt32(preferredResolution.width),
            height: UInt32(preferredResolution.height),
            framesPerSecond: 30,
            bitrateBps: activeBitrateBps,
            streamEpoch: camera.streamEpoch,
            mirrored: remoteMirrored,
            rotation: 0,
            sequenceParameterSet: Data(),
            pictureParameterSet: Data()
        )
    }

    func toggleResolution() async {
        let target: VideoResolution = switch camera.resolution {
        case .p1080: .p720
        case .p720: .p480
        case .p480: .p1080
        }
        await applyResolution(target)
    }

    func applySessionTick() {
        guard let session = senderSession else { return }
        var pumpError: String?
        do {
            try session.pump()
        } catch {
            pumpError = "连接出现异常，正在等待重新连接。"
        }

        let previousSenderStatus = senderStatus
        observeSession(session, pumpError: pumpError)
        if matchesActiveMediaState(previousSenderStatus),
           !matchesActiveMediaState
        {
            suspendMediaSending()
        }
        if sceneIsActive,
           previousSenderStatus == .reconnecting,
           senderStatus == .streaming
        {
            scheduleReconnectRebuild()
        }
        if bumpDiscoveryTick() {
            pollDiscovery()
        }
    }
}
