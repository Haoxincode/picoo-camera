import Foundation

// REQ-PICOO-MEDIA-011 / REQ-PICOO-PROTOCOL-005: encoded AUs are the only
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

    func consume(_ event: VideoEncoderEvent) throws {
        switch event {
        case let .failure(_, _, message):
            throw SenderMediaPipelineError.encoder(message)
        case let .accessUnit(accessUnit):
            try consume(accessUnit)
        case .queueOverflow:
            break
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

    private func consume(_ accessUnit: EncodedAccessUnit) throws {
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

        if updated != previous {
            try session.setStreamConfiguration(updated)
            // Submit reliable control before the corresponding datagram. QUIC
            // does not promise cross-channel ordering, so keyframes also carry
            // in-band SPS/PPS and Receiver gates mismatched epochs.
            try session.pump()
            configuration = updated
        }
        try session.send(accessUnit)
    }
}
