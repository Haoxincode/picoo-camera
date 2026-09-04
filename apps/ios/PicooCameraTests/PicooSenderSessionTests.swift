import Foundation
import Testing
@testable import PicooCamera

@Suite("Picoo iOS native boundaries")
struct PicooSenderSessionTests {
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
            try session.disconnect()
            #expect(session.snapshot.status == .disconnected)
        }

        #expect(releasedSession == nil)
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
        #expect(VideoResolution.supported(forRequestedHeight: 480) == .p480)
        #expect(VideoResolution.supported(forRequestedHeight: 720) == .p720)
        #expect(VideoResolution.supported(forRequestedHeight: 1080) == .p1080)
        #expect(VideoResolution.p1080.clamped(toMaximumHeight: 720) == .p720)
        #expect(VideoResolution.p1080.clamped(toMaximumHeight: 480) == .p480)
        #expect(VideoResolution.p720.clamped(toMaximumHeight: 0) == .p720)
        #expect(PicooSenderSession.clampBitrate(800_000, forHeight: 720) == 1_500_000)
        #expect(PicooSenderSession.clampBitrate(400_000, forHeight: 480) == 900_000)
        #expect(PicooSenderSession.clampBitrate(3_000_000, forHeight: 480) == 2_500_000)
        #expect(PicooSenderSession.clampBitrate(8_000_000, forHeight: 720) == 5_000_000)
        #expect(PicooSenderSession.clampBitrate(2_000_000, forHeight: 1080) == 3_000_000)
        #expect(PicooSenderSession.clampBitrate(12_000_000, forHeight: 1080) == 10_000_000)
    }

    @Test("stream epoch is monotonic and never overwrites a pending apply")
    func streamEpochPolicy() throws {
        let session = try PicooSenderSession(defaultDeviceName: "Epoch Testing")
        #expect(session.snapshot.streamEpoch == PicooSenderSession.initialStreamEpoch)
        let pending = session.beginStreamReconfiguration(targetHeight: 720)
        #expect(pending == PicooSenderSession.initialStreamEpoch + 1)
        #expect(session.beginStreamReconfiguration(targetHeight: 720) == 0)
        try session.cancelStreamReconfiguration(pending)
        let next = session.beginStreamReconfiguration(targetHeight: 720)
        #expect(next == pending + 1)
        try session.cancelStreamReconfiguration(next)
    }

    @Test("encoder configuration normalizes rotation and clamps bitrate")
    func encoderConfigurationPolicy() {
        let canonicalBitrate = PicooSenderSession.clampBitrate(
            12_000_000,
            forHeight: 1080
        )
        let configuration = VideoEncoderConfiguration(
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

    private func accessUnit(keyframe: Bool, pts: UInt64) -> EncodedAccessUnit {
        EncodedAccessUnit(
            data: Data([0, 0, 0, 1, keyframe ? 0x65 : 0x41]),
            isKeyframe: keyframe,
            presentationTimeUs: pts,
            width: 1280,
            height: 720,
            framesPerSecond: 30,
            bitrateBps: 3_000_000,
            streamEpoch: 1,
            encoderGeneration: 1,
            rotation: 0,
            parameterSets: nil
        )
    }
}
