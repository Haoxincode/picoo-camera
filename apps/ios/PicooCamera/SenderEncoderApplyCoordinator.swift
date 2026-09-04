import Foundation

struct PendingEncoderApply {
    let transactionID: UInt64
    let streamEpoch: UInt32
    let encoderGeneration: UInt64
    let targetHeight: UInt32
    let targetBitrateBps: UInt32
    let recoveryMessage: String?
}

struct CommittedEncoderState {
    let resolution: VideoResolution
    let position: CameraPosition
    let streamEpoch: UInt32
    let bitrateBps: UInt32
}

// REQ-PICOO-MEDIA-007/016: execute Rust effects and report native facts.
// Rust alone commits, rolls back, times out, requests recovery, or disconnects.
@MainActor
final class SenderEncoderApplyCoordinator {
    private static let recoveryDirectiveKind: UInt32 = 4

    private var pending: PendingEncoderApply?
    private var committed: CommittedEncoderState?
    private var recoveryTask: Task<Void, Never>?

    var isPending: Bool { pending != nil || recoveryTask != nil }

    deinit {
        recoveryTask?.cancel()
    }

    func waitForApply(
        directive: SenderEncoderDirective?,
        streamEpoch: UInt32,
        encoderGeneration: UInt64,
        height: UInt32,
        bitrateBps: UInt32,
        session: PicooSenderSession
    ) {
        let transactionID = directive?.id
            ?? session.encoderTransactionID(for: streamEpoch)
        pending = PendingEncoderApply(
            transactionID: transactionID,
            streamEpoch: streamEpoch,
            encoderGeneration: encoderGeneration,
            targetHeight: height,
            targetBitrateBps: bitrateBps,
            recoveryMessage: nil
        )
    }

    func beginLocal(session: PicooSenderSession, targetHeight: UInt32) -> UInt32 {
        recoveryTask?.cancel()
        recoveryTask = nil
        return session.beginStreamReconfiguration(targetHeight: targetHeight)
    }

    func accepts(_ accessUnit: EncodedAccessUnit) -> Bool {
        guard let pending else { return true }
        return accessUnit.isKeyframe
            && accessUnit.streamEpoch == pending.streamEpoch
            && accessUnit.encoderGeneration == pending.encoderGeneration
            && accessUnit.height == pending.targetHeight
    }

    func didCommit(_ accessUnit: EncodedAccessUnit, host: SenderAppModel) {
        guard let pending,
              accessUnit.streamEpoch == pending.streamEpoch,
              accessUnit.encoderGeneration == pending.encoderGeneration,
              accessUnit.height == pending.targetHeight
        else {
            return
        }
        host.commitAppliedEncoder(
            bitrateBps: pending.targetBitrateBps,
            recoveryMessage: pending.recoveryMessage
        )
        committed = CommittedEncoderState(
            resolution: VideoResolution.supported(forRequestedHeight: accessUnit.height),
            position: host.camera.position,
            streamEpoch: accessUnit.streamEpoch,
            bitrateBps: pending.targetBitrateBps
        )
        self.pending = nil
    }

    func handleFailure(_ event: VideoEncoderEvent, host: SenderAppModel) {
        guard case let .failure(streamEpoch, encoderGeneration, message) = event,
              let session = host.senderSession
        else {
            return
        }
        let outcome = session.reportEncoderFailed(
            streamEpoch: streamEpoch,
            encoderGeneration: encoderGeneration
        )
        handle(outcome: outcome, message: message, host: host)
    }

    func failBeforeStart(
        streamEpoch: UInt32,
        message: String,
        host: SenderAppModel
    ) {
        guard let session = host.senderSession else { return }
        let outcome = session.reportEncoderFailed(
            streamEpoch: streamEpoch,
            encoderGeneration: 0
        )
        handle(outcome: outcome, message: message, host: host)
    }

    func rejectBeforeStart(_ directive: SenderEncoderDirective, host: SenderAppModel) {
        guard let session = host.senderSession else { return }
        let outcome = session.reportEncoderFailed(
            transactionID: directive.id,
            encoderGeneration: 0
        )
        handle(
            outcome: outcome,
            message: "接收端能力不支持请求的 \(directive.targetHeight)P。",
            host: host
        )
    }

    func reconcileCore(host: SenderAppModel) {
        guard recoveryTask == nil, let session = host.senderSession else { return }
        if let pending,
           session.encoderTransactionID(for: pending.streamEpoch) == pending.transactionID
        {
            return
        }
        if let directive = try? session.encoderDirective(),
           directive.kind == Self.recoveryDirectiveKind
        {
            scheduleRecovery(
                directive: directive,
                after: pending?.recoveryMessage ?? "编码器调整超时",
                host: host
            )
            return
        }
        if session.snapshot.status == .disconnected {
            pending = nil
        }
    }

    func cancelPending(session: PicooSenderSession?) {
        if let pending, let session {
            _ = session.reportEncoderFailed(
                transactionID: pending.transactionID,
                encoderGeneration: 0
            )
        }
        pending = nil
    }

    func clearPending() {
        pending = nil
    }

    func cancelRecovery() {
        recoveryTask?.cancel()
        recoveryTask = nil
    }

    private func handle(
        outcome: SenderEncoderFailureOutcome,
        message: String,
        host: SenderAppModel
    ) {
        switch outcome {
        case .ignored:
            return
        case .rolledBack:
            pending = nil
            host.resumeMediaAfterEncoderRollback(message: message)
        case .recoveryRequested:
            guard let directive = try? host.senderSession?.encoderDirective(),
                  directive.kind == Self.recoveryDirectiveKind
            else {
                pending = nil
                host.noteError("\(message)；无法取得恢复配置。")
                return
            }
            scheduleRecovery(directive: directive, after: message, host: host)
        case .disconnected:
            pending = nil
            host.disconnectImmediately()
            host.noteError("\(message)；恢复上一视频配置失败，已断开连接。")
        }
    }

    private func scheduleRecovery(
        directive: SenderEncoderDirective,
        after message: String,
        host: SenderAppModel
    ) {
        recoveryTask?.cancel()
        pending = nil
        guard let committed,
              committed.streamEpoch == directive.streamEpoch,
              UInt32(committed.resolution.rawValue) == directive.targetHeight
        else {
            _ = host.senderSession?.reportEncoderFailed(
                transactionID: directive.id,
                encoderGeneration: 0
            )
            host.disconnectImmediately()
            host.noteError("\(message)；尚无可恢复的视频配置，已断开连接。")
            return
        }
        recoveryTask = Task { [weak host, weak self] in
            guard let host, let self else { return }
            let restored = await host.camera.restoreCommittedConfiguration(
                resolution: committed.resolution,
                position: committed.position,
                bitrateBps: directive.targetBitrateBps,
                streamEpoch: directive.streamEpoch
            )
            guard !Task.isCancelled else { return }
            self.recoveryTask = nil
            guard restored else {
                let outcome = host.senderSession?.reportEncoderFailed(
                    transactionID: directive.id,
                    encoderGeneration: 0
                ) ?? .ignored
                self.handle(outcome: outcome, message: message, host: host)
                return
            }
            self.pending = PendingEncoderApply(
                transactionID: directive.id,
                streamEpoch: directive.streamEpoch,
                encoderGeneration: host.camera.encoderGeneration,
                targetHeight: directive.targetHeight,
                targetBitrateBps: directive.targetBitrateBps,
                recoveryMessage: message
            )
            await host.camera.requestKeyframe()
        }
    }
}
