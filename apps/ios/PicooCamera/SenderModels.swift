import Foundation
import Network

// REQ-PICOO-UI-010 / REQ-PICOO-DISCOVERY-008

nonisolated enum PicooSenderStatus: Int32, Sendable {
    case disconnected = 0
    case discovering = 1
    case pairing = 2
    case connecting = 3
    case negotiating = 4
    case streaming = 5
    case reconnecting = 6
    case permissionRequired = 7
    case networkUnstable = 8

    init(code: Int32) {
        self = Self(rawValue: code) ?? .disconnected
    }
}

nonisolated struct ReceiverEndpoint: Equatable, Sendable {
    let host: String
    let port: UInt16

    init(host: String, port: UInt16) {
        self.host = host
        self.port = port
    }

    init?(manualText: String) {
        let value = manualText.trimmingCharacters(in: .whitespacesAndNewlines)
        let host: String
        let portText: String

        if value.hasPrefix("["), let closingBracket = value.firstIndex(of: "]") {
            host = String(value[value.index(after: value.startIndex)..<closingBracket])
            let suffix = value[value.index(after: closingBracket)...]
            guard suffix.hasPrefix(":"), suffix.count > 1 else { return nil }
            portText = String(suffix.dropFirst())
            guard Self.isIPv6(host) else { return nil }
        } else {
            guard let separator = value.lastIndex(of: ":") else { return nil }
            host = String(value[..<separator])
            portText = String(value[value.index(after: separator)...])
            guard Self.isIPv4(host) else { return nil }
        }

        guard let port = UInt16(portText), port > 0 else { return nil }
        self.host = host
        self.port = port
    }

    var displayText: String {
        host.contains(":") ? "[\(host)]:\(port)" : "\(host):\(port)"
    }

    private static func isIPv4(_ value: String) -> Bool {
        IPv4Address(value) != nil
    }

    private static func isIPv6(_ value: String) -> Bool {
        IPv6Address(value) != nil
    }
}

nonisolated struct ReceiverSummary: Identifiable, Equatable, Sendable {
    let id: String
    let displayName: String
    let endpoint: ReceiverEndpoint
    let pairingState: String?
    let isTrusted: Bool
}

nonisolated enum SenderScreen: Equatable, Sendable {
    case devices
    case pairing
    case waiting
    case live
}

nonisolated enum SenderScreenResolver {
    static func resolve(
        status: PicooSenderStatus,
        pairingCode: String,
        phoneConfirmedPairing: Bool
    ) -> SenderScreen {
        switch status {
        case .streaming, .reconnecting, .permissionRequired, .networkUnstable:
            return .live
        case .negotiating:
            return .waiting
        case .pairing where phoneConfirmedPairing:
            return .waiting
        case .pairing:
            return .pairing
        case .connecting where phoneConfirmedPairing:
            return .waiting
        case .connecting, .discovering, .disconnected:
            return pairingCode.isEmpty ? .devices : .pairing
        }
    }
}
