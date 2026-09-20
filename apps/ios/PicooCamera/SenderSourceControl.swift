import Foundation

extension SenderAppModel {
    func applySourceFormat(
        _ source: VideoSourceFormat,
        directive: SenderEncoderDirective? = nil,
        captureRotation: UInt32? = nil
    ) async {
        guard let session = senderSession else { return }
        guard session.snapshot.receiverSourceFormats?.contains(source) == true else {
            noteError("接收端不支持所选视频配置。")
            if let directive { encoderApply.rejectBeforeStart(directive, host: self) }
            return
        }
        let beforePosition = camera.position
        let beforeSource = camera.sourceFormat
        let beforeEpoch = session.snapshot.streamEpoch
        await camera.refreshSourceFormats()
        if let captureRotation {
            guard camera.position == beforePosition, camera.sourceFormat == beforeSource,
                  session.snapshot.streamEpoch == beforeEpoch, matchesActiveMediaState,
                  camera.requestedCaptureRotation == captureRotation else { return }
        }
        guard !Task.isCancelled, availableSourceFormats?.contains(source) == true else {
            noteError("当前镜头无法准备所选视频格式。")
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
            noteError("接收端要求先完成当前编码器调整。")
            return
        }
        let applied: Bool
        if camera.state == .running {
            applied = await camera.setSourceFormat(source, captureRotation: captureRotation ?? camera.captureRotation, bitrateBps: targetBitrate, streamEpoch: streamEpoch)
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
            captureRotation: camera.captureRotation,
            bitrateBps: targetBitrate,
            session: session
        )
    }

}
