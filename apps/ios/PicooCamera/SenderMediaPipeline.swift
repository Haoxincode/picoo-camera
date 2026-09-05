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

    init(session: PicooSenderSession) {
        self.session = session
    }

    func prime(
        resolution: VideoResolution,
        bitrateBps: UInt32,
        streamEpoch: UInt32,
        mirrored: Bool
    ) throws {
        let previous = configuration
        let dimensionsAreUnchanged = previous?.streamEpoch == streamEpoch
            && previous?.width == UInt32(resolution.width)
            && previous?.height == UInt32(resolution.height)
        let configuration = SenderStreamConfiguration(
            width: UInt32(resolution.width),
            height: UInt32(resolution.height),
            framesPerSecond: 30,
            bitrateBps: bitrateBps,
            streamEpoch: streamEpoch,
            mirrored: mirrored,
            rotation: dimensionsAreUnchanged ? previous?.rotation ?? 0 : 0,
            sequenceParameterSet: dimensionsAreUnchanged
                ? previous?.sequenceParameterSet ?? Data()
                : Data(),
            pictureParameterSet: dimensionsAreUnchanged
                ? previous?.pictureParameterSet ?? Data()
                : Data()
        )
        try session.setStreamConfiguration(configuration)
        self.configuration = configuration
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

    func setMirrored(_ mirrored: Bool) throws {
        guard let configuration else { return }
        let updated = SenderStreamConfiguration(
            width: configuration.width,
            height: configuration.height,
            framesPerSecond: configuration.framesPerSecond,
            bitrateBps: configuration.bitrateBps,
            streamEpoch: configuration.streamEpoch,
            mirrored: mirrored,
            rotation: configuration.rotation,
            sequenceParameterSet: configuration.sequenceParameterSet,
            pictureParameterSet: configuration.pictureParameterSet
        )
        try session.setStreamConfiguration(updated)
        self.configuration = updated
    }

    private func consume(_ accessUnit: EncodedAccessUnit) throws -> EncoderSubmitResult {
        let previous = configuration
        let parameterSets = accessUnit.parameterSets
        let updated = SenderStreamConfiguration(
            width: accessUnit.width,
            height: accessUnit.height,
            framesPerSecond: accessUnit.framesPerSecond,
            bitrateBps: accessUnit.bitrateBps,
            streamEpoch: accessUnit.streamEpoch,
            mirrored: previous?.mirrored ?? false,
            rotation: accessUnit.rotation,
            sequenceParameterSet: parameterSets?.sequence
                ?? previous?.sequenceParameterSet
                ?? Data(),
            pictureParameterSet: parameterSets?.picture
                ?? previous?.pictureParameterSet
                ?? Data()
        )

        let configurationChanged = updated != previous
        let result = try session.send(
            accessUnit,
            streamConfiguration: configurationChanged ? updated : nil
        )
        if configurationChanged && result.streamConfigured {
            configuration = updated
        }
        return result
    }
}
