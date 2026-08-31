import Foundation

struct PendingEncoderApply {
    let directive: SenderEncoderDirective?
    let streamEpoch: UInt32
    let encoderGeneration: UInt64
    let targetHeight: UInt32
    let targetBitrateBps: UInt32
    let deadline: ContinuousClock.Instant
    let recoveryMessage: String?
}

struct CommittedEncoderState {
    let resolution: VideoResolution
    let position: CameraPosition
    let streamEpoch: UInt32
    let bitrateBps: UInt32
}

// REQ-PICOO-MEDIA-007: native-encoder apply/ACK lives here, not in SwiftUI state.
@MainActor
final class SenderEncoderApplyCoordinator {
    private var pending: PendingEncoderApply?
    private var committed: CommittedEncoderState?
    private var recoveryTask: Task<Void, Never>?

    var isPending: Bool { pending != nil }

    deinit {
        recoveryTask?.cancel()
    }

    func waitForApply(
        directive: SenderEncoderDirective?,
        streamEpoch: UInt32,
        encoderGeneration: UInt64,
        height: UInt32,
        bitrateBps: UInt32
    ) {
        pending = PendingEncoderApply(
            directive: directive,
            streamEpoch: streamEpoch,
            encoderGeneration: encoderGeneration,
            targetHeight: height,
            targetBitrateBps: bitrateBps,
            deadline: ContinuousClock.now.advanced(by: .seconds(3)),
            recoveryMessage: nil
        )
    }

    func beginLocal(session: PicooSenderSession) -> UInt32 {
        recoveryTask?.cancel()
        recoveryTask = nil
        cancelPending(session: session)
        if let queuedDirective = try? session.encoderDirective() {
            try? session.rejectEncoderDirective(queuedDirective.id)
        }
        return session.beginStreamReconfiguration()
    }

    func complete(with event: VideoEncoderEvent, host: SenderAppModel) -> Bool {
        guard let pending, let session = host.senderSession else { return false }
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
                host.commitAppliedEncoder(
                    bitrateBps: pending.targetBitrateBps,
                    recoveryMessage: pending.recoveryMessage
                )
                committed = CommittedEncoderState(
                    resolution: VideoResolution.supported(
                        forRequestedHeight: accessUnit.height
                    ),
                    position: host.camera.position,
                    streamEpoch: accessUnit.streamEpoch,
                    bitrateBps: pending.targetBitrateBps
                )
                self.pending = nil
                return true
            } catch {
                failPending(host: host, message: "无法同步视频分辨率。")
                return false
            }
        case let .failure(_, _, message):
            failPending(host: host, message: message)
            return false
        case .queueOverflow:
            return false
        }
    }

    func expireIfNeeded(host: SenderAppModel) {
        guard let pending,
              ContinuousClock.now >= pending.deadline
        else {
            return
        }
        failPending(host: host, message: "编码器未在 3 秒内输出目标关键帧。")
    }

    func failPending(host: SenderAppModel, message: String) {
        guard let pending, let session = host.senderSession else { return }
        if let directive = pending.directive {
            try? session.rejectEncoderDirective(directive.id)
        } else {
            try? session.cancelStreamReconfiguration(pending.streamEpoch)
        }
        self.pending = nil
        host.disableMediaSending()
        if pending.recoveryMessage != nil {
            host.disconnectImmediately()
            host.noteError("\(message)；恢复上一视频配置失败，已断开连接。")
            return
        }
        scheduleRecovery(after: message, host: host)
    }

    func cancelPending(session: PicooSenderSession?) {
        guard let pending, let session else { return }
        if let directive = pending.directive {
            try? session.rejectEncoderDirective(directive.id)
        } else {
            try? session.cancelStreamReconfiguration(pending.streamEpoch)
        }
        self.pending = nil
    }

    func clearPending() {
        pending = nil
    }

    func cancelRecovery() {
        recoveryTask?.cancel()
        recoveryTask = nil
    }

    func scheduleRecovery(after message: String, host: SenderAppModel) {
        recoveryTask?.cancel()
        guard let committed else {
            host.disconnectImmediately()
            host.noteError("\(message)；尚无可恢复的视频配置，已断开连接。")
            return
        }
        recoveryTask = Task { [weak host, weak self] in
            guard let host, let self else { return }
            let restored = await host.camera.restoreCommittedConfiguration(
                resolution: committed.resolution,
                position: committed.position,
                bitrateBps: committed.bitrateBps,
                streamEpoch: committed.streamEpoch
            )
            guard !Task.isCancelled else { return }
            self.recoveryTask = nil
            guard restored else {
                host.disconnectImmediately()
                host.noteError("\(message)；恢复上一视频配置失败，已断开连接。")
                return
            }
            self.pending = PendingEncoderApply(
                directive: nil,
                streamEpoch: committed.streamEpoch,
                encoderGeneration: host.camera.encoderGeneration,
                targetHeight: UInt32(committed.resolution.rawValue),
                targetBitrateBps: committed.bitrateBps,
                deadline: ContinuousClock.now.advanced(by: .seconds(3)),
                recoveryMessage: message
            )
            await host.camera.requestKeyframe()
        }
    }
}
