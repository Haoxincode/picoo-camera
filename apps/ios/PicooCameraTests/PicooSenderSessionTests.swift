import Foundation
import CoreMedia
import PicooCore
import Testing
@testable import PicooCamera

@Suite("Picoo iOS native boundaries")
struct PicooSenderSessionTests {
    @Test("C fixed-array candidates preserve codec and frame rate")
    func decoderCandidatesCrossSnapshotBoundary() throws {
        var value = PicooSenderSnapshot()
        #expect(PicooSenderSession.sourceFormats(from: value) == nil)
        value.receiver_capabilities_known = true
        #expect(PicooSenderSession.sourceFormats(from: value) == [])
        value.receiver_source_format_count = 2
        value.receiver_source_formats.0 = PicooSourceFormat(codec: 1, height: 720, fps: 30)
        value.receiver_source_formats.1 = PicooSourceFormat(codec: 2, height: 1080, fps: 60)
        let formats = try #require(PicooSenderSession.sourceFormats(from: value))
        #expect(formats.count == 2)
        #expect(formats[0].codec == .avc && formats[0].resolution == .p720 && formats[0].framesPerSecond == 30)
        #expect(formats[1].codec == .hevc && formats[1].resolution == .p1080 && formats[1].framesPerSecond == 60)
    }

    @Test("Rust protocol name crosses the C ABI")
    func protocolNameCrossesSwiftCAbiBoundary() {
        #expect(PicooSenderSession.protocolName == "PCP")
    }

    @Test("Sender handle begins disconnected and has deterministic ownership")
    func senderHandleStartsDisconnectedAndOwnerReleases() throws {
        weak var releasedSession: PicooSenderSession?

        try autoreleasepool {
            let session = try PicooSenderSession(defaultDeviceName: "Swift Testing")
            releasedSession = session
            #expect(session.snapshot.status == .disconnected)
            #expect(session.snapshot.receiverSourceFormats == nil)
            try session.disconnect()
            #expect(session.snapshot.status == .disconnected)
        }

        #expect(releasedSession == nil)
    }

    @Test("Sender identity survives a session restart in the iOS Keychain")
    func senderIdentityPersistsAcrossSessionRestart() throws {
        let firstID = try autoreleasepool {
            try PicooSenderSession(defaultDeviceName: "Keychain First").localDeviceID
        }
        let secondID = try autoreleasepool {
            try PicooSenderSession(defaultDeviceName: "Keychain Second").localDeviceID
        }

        #expect(firstID.hasPrefix("picoo-"))
        #expect(secondID == firstID)
    }

    @Test(
        "Manual endpoints accept explicit local IPv4 and bracketed IPv6",
        arguments: [
            ("192.168.1.108:4433", "192.168.1.108:4433"),
            ("[fd00::108]:4433", "[fd00::108]:4433"),
        ]
    )
    func acceptsManualEndpoint(input: String, expected: String) {
        #expect(ReceiverEndpoint(manualText: input)?.displayText == expected)
    }

    @Test(
        "Manual endpoints reject ambiguous or invalid input",
        arguments: [
            "192.168.1.108",
            "192.168.1.999:4433",
            "192.168.1.108:0",
            "fd00::108:4433",
            "[:::]:4433",
            "picoo.local:4433",
        ]
    )
    func rejectsInvalidManualEndpoint(input: String) {
        #expect(ReceiverEndpoint(manualText: input) == nil)
    }

    @Test("Manual endpoint paste distributes IPv4 and keeps the editable port")
    func manualEndpointPasteDistribution() {
        let draft = ManualEndpointDraft(pastedText: "192.168.1.108:8443")

        #expect(draft?.octets == ["192", "168", "1", "108"])
        #expect(draft?.port == "8443")
        #expect(draft?.text == "192.168.1.108:8443")
        #expect(ManualEndpointDraft(pastedText: "192.168.1.108.7:4433") == nil)
    }

    @Test("Manual endpoint fields prefill the Picoo QUIC port")
    func manualEndpointDefaultPort() {
        let draft = ManualEndpointDraft(text: "")

        #expect(draft.octets == ["", "", "", ""])
        #expect(draft.port == "4433")
        #expect(ManualEndpointDraft(text: "192.168.1.108:").port.isEmpty)
        #expect(ManualEndpointDraft.shouldAdvance(octet: "192"))
        #expect(ManualEndpointDraft.shouldAdvance(octet: "26"))
    }

    @Test("Sender status snapshots resolve to stable SwiftUI screens")
    func resolvesSenderScreens() {
        #expect(SenderScreenResolver.resolve(
            status: .disconnected,
            pairingCode: "",
            phoneConfirmedPairing: false
        ) == .devices)
        #expect(SenderScreenResolver.resolve(
            status: .pairing,
            pairingCode: "482917",
            phoneConfirmedPairing: false
        ) == .pairing)
        #expect(SenderScreenResolver.resolve(
            status: .pairing,
            pairingCode: "482917",
            phoneConfirmedPairing: true
        ) == .waiting)
        #expect(SenderScreenResolver.resolve(
            status: .negotiating,
            pairingCode: "",
            phoneConfirmedPairing: true
        ) == .waiting)
        #expect(SenderScreenResolver.resolve(
            status: .streaming,
            pairingCode: "",
            phoneConfirmedPairing: true
        ) == .live)
    }

    @Test("iOS encoder policy keeps negotiated resolution and bitrate in supported bounds")
    func encoderPolicyBounds() {
        for height: UInt32 in [0, 480, 719, 721, 1079, 1081, 2160, UInt32.max] {
            #expect(VideoResolution.supported(forRequestedHeight: height) == nil)
            #expect(PicooSenderSession.initialBitrate(forHeight: height) == 0)
        }
        #expect(VideoResolution.supported(forRequestedHeight: 720) == .p720)
        #expect(VideoResolution.supported(forRequestedHeight: 1080) == .p1080)
        #expect(PicooSenderSession.clampBitrate(800_000, forHeight: 720) == 1_500_000)
        #expect(PicooSenderSession.clampBitrate(400_000, forHeight: 480) == 0)
        #expect(PicooSenderSession.clampBitrate(3_000_000, forHeight: 480) == 0)
        #expect(PicooSenderSession.clampBitrate(8_000_000, forHeight: 720) == 5_000_000)
        #expect(PicooSenderSession.clampBitrate(2_000_000, forHeight: 1080) == 3_000_000)
        #expect(PicooSenderSession.clampBitrate(12_000_000, forHeight: 1080) == 10_000_000)
    }

    @Test("stream epoch is monotonic and never overwrites a pending apply")
    func streamEpochPolicy() throws {
        let session = try PicooSenderSession(defaultDeviceName: "Epoch Testing")
        #expect(session.snapshot.streamEpoch == PicooSenderSession.initialStreamEpoch)
        let pending = session.beginStreamReconfiguration(targetHeight: 720, codec: 1, framesPerSecond: 30)
        #expect(pending == PicooSenderSession.initialStreamEpoch + 1)
        #expect(session.beginStreamReconfiguration(targetHeight: 720, codec: 1, framesPerSecond: 30) == 0)
        #expect(session.reportEncoderFailed(
            streamEpoch: pending,
            encoderGeneration: 0
        ) == .rolledBack)
        let next = session.beginStreamReconfiguration(targetHeight: 720, codec: 1, framesPerSecond: 30)
        #expect(next == pending + 1)
        #expect(session.reportEncoderFailed(
            streamEpoch: next,
            encoderGeneration: 0
        ) == .rolledBack)
    }

    @Test("explicit codec and frame rate are admitted at the C request boundary")
    func sourceFormatRequestRoundTrip() throws {
        let session = try PicooSenderSession(defaultDeviceName: "Format Testing")
        #expect(session.beginStreamReconfiguration(targetHeight: 1080, codec: 0, framesPerSecond: 60) == 0)
        #expect(session.beginStreamReconfiguration(targetHeight: 1080, codec: 2, framesPerSecond: 120) == 0)
        let epoch = session.beginStreamReconfiguration(targetHeight: 1080, codec: 2, framesPerSecond: 60)
        #expect(epoch > PicooSenderSession.initialStreamEpoch)
        // Local requests are already being applied by this owner. The effect
        // getter exposes recovery commands, not a duplicate local apply.
        #expect(try session.encoderDirective() == nil)
        #expect(session.encoderTransactionID(for: epoch) > 0)
        #expect(session.reportEncoderFailed(streamEpoch: epoch, encoderGeneration: 0) == .rolledBack)
    }

    @Test("encoder configuration normalizes rotation and clamps bitrate")
    func encoderConfigurationPolicy() {
        let canonicalBitrate = PicooSenderSession.clampBitrate(
            12_000_000,
            forHeight: 1080
        )
        let configuration = VideoEncoderConfiguration(
            codec: .avc,
            resolution: .p1080,
            bitrateBps: canonicalBitrate,
            streamEpoch: 7,
            encoderGeneration: 11,
            rotation: 450
        )
        #expect(configuration.bitrateBps == canonicalBitrate)
        #expect(configuration.streamEpoch == 7)
        #expect(configuration.encoderGeneration == 11)
        #expect(configuration.rotation == 90)
    }

    @Test("a successful C call keeps rejected encoder facts rejected in Swift")
    func rejectedEncoderOutcomeRemainsRejected() {
        let result = EncoderSubmitResult(
            cEncoderAccepted: 0,
            cStreamConfigured: 0,
            cKeyframeRequested: 1,
            packetCount: 0,
            sentCount: 0
        )

        #expect(!result.encoderAccepted)
        #expect(!result.streamConfigured)
        #expect(result.keyframeRequested)
        #expect(result.packetCount == 0)
        #expect(result.sentCount == 0)
    }

    @Test("bounded encoder queue drops dependent frames until a fresh IDR")
    func encoderQueueRecoversAtKeyframe() {
        let buffer = VideoEncoderEventBuffer(capacity: 2)
        buffer.enqueue(.accessUnit(accessUnit(keyframe: true, pts: 1)))
        buffer.enqueue(.accessUnit(accessUnit(keyframe: false, pts: 2)))
        buffer.enqueue(.accessUnit(accessUnit(keyframe: false, pts: 3)))

        let overflow = buffer.drain()
        #expect(overflow.count == 1)
        if case .queueOverflow = overflow.first {
            // Expected: the consumer requests a new IDR.
        } else {
            Issue.record("queue overflow marker missing")
        }

        buffer.enqueue(.accessUnit(accessUnit(keyframe: false, pts: 4)))
        #expect(buffer.drain().isEmpty)
        buffer.enqueue(.accessUnit(accessUnit(keyframe: true, pts: 5)))
        let recovered = buffer.drain()
        #expect(recovered.count == 1)
        if case let .accessUnit(accessUnit) = recovered.first {
            #expect(accessUnit.isKeyframe)
            #expect(accessUnit.presentationTimeUs == 5)
        } else {
            Issue.record("fresh IDR did not reopen the queue")
        }
    }

    @Test("Native format codec and record remain paired in the callback snapshot")
    func nativeConfigurationSnapshot() throws {
        for (codec, wire, atom) in [(kCMVideoCodecType_H264, UInt32(1), "avcC"),
                                    (kCMVideoCodecType_HEVC, UInt32(2), "hvcC")] {
            let record = Data([1, 2, 3, 4])
            var format: CMFormatDescription?
            let extensions = [kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms: [atom: record]] as CFDictionary
            let status = CMVideoFormatDescriptionCreate(allocator: nil, codecType: codec,
                width: 64, height: 64, extensions: extensions, formatDescriptionOut: &format)
            #expect(status == noErr)
            let snapshot = try CompressionCallbackContext.codecConfiguration(from: format)
            #expect(snapshot.codec == wire)
            #expect(snapshot.record == record)
        }
    }

    @Test("Missing native format never produces an empty source configuration")
    func missingNativeConfigurationIsRejected() {
        #expect(throws: (any Error).self) {
            try CompressionCallbackContext.codecConfiguration(from: nil)
        }
    }

    @Test("encoder completions retain input facts across updates and out-of-order callbacks")
    func encoderCompletionUsesOriginalSubmission() throws {
        let pending = SubmittedFrameConfigurations()
        let first = EncodedFrameConfiguration(
            codec: .avc,
            width: 1280, height: 720, framesPerSecond: 30, bitrateBps: 3_000_000,
            streamEpoch: 2, encoderGeneration: 3, rotation: 0
        )
        let next = EncodedFrameConfiguration(
            codec: .avc,
            width: 1280, height: 720, framesPerSecond: 30, bitrateBps: 4_000_000,
            streamEpoch: 2, encoderGeneration: 3, rotation: 90
        )
        let firstID = try #require(pending.reserve(first))
        let nextID = try #require(pending.reserve(next))
        #expect(pending.take(nextID) == next)
        #expect(pending.take(nextID) == nil)
        #expect(pending.take(0) == nil)
        #expect(pending.take(firstID) == first)
    }

    @Test("pending native frames are bounded and cancelled IDs are never reused")
    func encoderCompletionCapacityAndCancellation() throws {
        let pending = SubmittedFrameConfigurations()
        let frame = EncodedFrameConfiguration(
            codec: .avc,
            width: 1920, height: 1080, framesPerSecond: 60, bitrateBps: 5_000_000,
            streamEpoch: 7, encoderGeneration: 9, rotation: 180
        )
        var identifiers: [UInt] = []
        for _ in 0..<16 { identifiers.append(try #require(pending.reserve(frame))) }
        #expect(pending.reserve(frame) == nil)
        #expect(pending.take(identifiers[0]) == frame)
        #expect(pending.take(identifiers[0]) == nil)
        let replacement = try #require(pending.reserve(frame))
        #expect(!identifiers.contains(replacement))
        #expect(pending.take(identifiers[0]) == nil)
        #expect(pending.take(replacement) == frame)
        for identifier in identifiers.dropFirst() { #expect(pending.take(identifier) == frame) }
    }

    private func accessUnit(keyframe: Bool, pts: UInt64) -> EncodedAccessUnit {
        EncodedAccessUnit(
            data: Data([0, 0, 0, 1, keyframe ? 0x65 : 0x41]),
            isKeyframe: keyframe,
            presentationTimeUs: pts,
            encodedAtUs: pts,
            width: 1280,
            height: 720,
            framesPerSecond: 30,
            bitrateBps: 3_000_000,
            streamEpoch: 1,
            encoderGeneration: 1,
            rotation: 0,
            codecConfiguration: EncodedCodecConfiguration(codec: 1, record: Data([1]))
        )
    }
}
