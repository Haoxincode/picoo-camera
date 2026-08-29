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
}
