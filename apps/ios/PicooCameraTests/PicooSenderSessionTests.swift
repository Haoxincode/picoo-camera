import Foundation
import Testing
@testable import PicooCamera

@Suite("Picoo iOS native boundaries")
struct PicooSenderSessionTests {
    @Test("Rust protocol version crosses the C ABI")
    func protocolVersionCrossesSwiftCAbiBoundary() {
        #expect(PicooSenderSession.protocolVersion == "PCP/1")
    }

    @Test("Sender handle begins disconnected and has deterministic ownership")
    func senderHandleStartsDisconnectedAndOwnerReleases() throws {
        weak var releasedSession: PicooSenderSession?

        try autoreleasepool {
            let session = try PicooSenderSession(defaultDeviceName: "Swift Testing")
            releasedSession = session
            #expect(session.statusCode == PicooSenderStatus.disconnected.rawValue)
            try session.disconnect()
            #expect(session.statusCode == PicooSenderStatus.disconnected.rawValue)
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
        #expect(VideoBitrate.clamp(800_000, for: .p720) == 1_500_000)
        #expect(VideoBitrate.clamp(400_000, for: .p480) == 900_000)
        #expect(VideoBitrate.clamp(3_000_000, for: .p480) == 2_500_000)
        #expect(VideoBitrate.clamp(8_000_000, for: .p720) == 5_000_000)
        #expect(VideoBitrate.clamp(2_000_000, for: .p1080) == 3_000_000)
        #expect(VideoBitrate.clamp(12_000_000, for: .p1080) == 10_000_000)
    }

    @Test("stream epoch is monotonic and wraps without using zero")
    func streamEpochPolicy() {
        #expect(StreamEpoch.bump(StreamEpoch.initial) == 2)
        #expect(StreamEpoch.bump(.max) == StreamEpoch.initial)
    }

    @Test("encoder configuration normalizes rotation and clamps bitrate")
    func encoderConfigurationPolicy() {
        let configuration = VideoEncoderConfiguration(
            resolution: .p1080,
            bitrateBps: 12_000_000,
            streamEpoch: 7,
            rotation: 450
        )
        #expect(configuration.bitrateBps == 10_000_000)
        #expect(configuration.streamEpoch == 7)
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
            rotation: 0,
            parameterSets: nil
        )
    }
}
