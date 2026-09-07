import Foundation

extension SenderAppModel {
    func preparedCameraSwitchSource() async -> VideoSourceFormat? {
        guard let session = senderSession, let remote = session.snapshot.receiverSourceFormats else { return nil }
        let before = camera.sourceFormat
        let position = camera.position
        let epoch = session.snapshot.streamEpoch
        let local = await camera.preparedSourceFormats(at: position.opposite)
        guard !Task.isCancelled, camera.position == position, camera.sourceFormat == before,
              session.snapshot.streamEpoch == epoch, matchesActiveMediaState else { return nil }
        return VideoSourceFormat.cameraCeiling(local.filter { remote.contains($0) }, preferredCodec: before.codec)
    }

    var availableSourceFormats: [VideoSourceFormat]? {
        guard let local = camera.localSourceFormats else { return nil }
        if senderStatus == .disconnected { return local }
        guard let remote = receiverSourceFormats else { return nil }
        return local.filter { remote.contains($0) }
    }

    var sourceFormatLabel: String {
        senderSession?.snapshot.lastCommittedSourceFormat?.label ?? "等待视频提交"
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
        if discoveryPollIsDue() {
            pollDiscovery()
        }
    }
}
