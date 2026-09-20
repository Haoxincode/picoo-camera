import Foundation

// REQ-PICOO-MEDIA-011/022 / REQ-PICOO-PROTOCOL-005: encoded AUs are the only
// camera payload crossing Swift -> Rust. Raw pixel buffers stay in AVFoundation.

nonisolated enum SenderMediaPipelineError: LocalizedError {
    case encoder(String)

    var errorDescription: String? {
        switch self {
        case let .encoder(message): message
        }
    }
}

actor SenderMediaPipeline {
    private let session: PicooSenderSession
    private var configuration: SenderStreamConfiguration?
    private var mirrored = false
    private var configuredEncoderGeneration: UInt64 = 0

    init(session: PicooSenderSession) {
        self.session = session
    }

    func consume(_ event: VideoEncoderEvent) throws -> EncoderSubmitResult {
        switch event {
        case let .failure(_, _, message):
            throw SenderMediaPipelineError.encoder(message)
        case let .accessUnit(accessUnit):
            return try consume(accessUnit)
        case .queueOverflow:
            return .ignored
        }
    }

    func setMirrored(_ mirrored: Bool) {
        self.mirrored = mirrored
    }

    private func consume(_ accessUnit: EncodedAccessUnit) throws -> EncoderSubmitResult {
        let previous = configuration
        let updated = SenderStreamConfiguration(
            width: accessUnit.width,
            height: accessUnit.height,
            framesPerSecond: accessUnit.framesPerSecond,
            bitrateBps: accessUnit.bitrateBps,
            streamEpoch: accessUnit.streamEpoch,
            mirrored: mirrored,
            rotation: accessUnit.rotation,
            codecConfiguration: accessUnit.codecConfiguration
        )

        let configurationChanged = updated != previous
            || configuredEncoderGeneration != accessUnit.encoderGeneration
        let result = try session.send(
            accessUnit,
            streamConfiguration: configurationChanged ? updated : nil
        )
        if configurationChanged && result.streamConfigured && result.encoderAccepted {
            configuration = updated
            configuredEncoderGeneration = accessUnit.encoderGeneration
        }
        return result
    }
}
