import Foundation
import PicooCore

enum PicooSenderSessionError: Error, Equatable {
    case senderCreationFailed
    case identityCreationFailed
    case discoveryCreationFailed
    case storageUnavailable
    case operationFailed(name: String, code: Int32)
}

/// Swift-owned lifetime boundary for the opaque Rust Sender handle.
///
/// Raw pointers never escape this type. Platform camera code hands encoded
/// H.264 access units to this session in REQ-PICOO-MEDIA-011; capture buffers do
/// not cross the FFI boundary.
final class PicooSenderSession {
    private let sender: UnsafeMutableRawPointer
    private let identity: PicooSenderIdentity
    private let trustedStorePath: String

    static var protocolVersion: String {
        guard let version = picoo_protocol_version() else {
            return "Unavailable"
        }
        return String(cString: version)
    }

    init(defaultDeviceName: String = "iPhone") throws {
        let storage = try Self.storageDirectory()
        trustedStorePath = storage.appendingPathComponent("trusted_devices.json").path
        identity = try PicooSenderIdentity(
            path: storage.appendingPathComponent("sender_identity.json").path,
            defaultDeviceName: defaultDeviceName
        )

        guard let senderHandle = picoo_sender_create() else {
            throw PicooSenderSessionError.senderCreationFailed
        }

        let attachCode = trustedStorePath.withCString { path in
            picoo_sender_attach_trusted_store(senderHandle, path)
        }
        guard attachCode >= 0 else {
            picoo_sender_destroy(senderHandle)
            throw PicooSenderSessionError.operationFailed(
                name: "sender_attach_trusted_store",
                code: attachCode
            )
        }
        sender = senderHandle
    }

    isolated deinit {
        picoo_sender_destroy(sender)
    }

    var status: PicooSenderStatus {
        PicooSenderStatus(code: picoo_sender_status(sender))
    }

    var statusCode: Int32 {
        status.rawValue
    }

    var pairingShortCode: String {
        readCString(maxLength: 16) { buffer, length in
            picoo_sender_pairing_short_code(sender, buffer, length)
        }
    }

    var connectedReceiverID: String {
        readCString(maxLength: 128) { buffer, length in
            picoo_sender_connected_receiver_id(sender, buffer, length)
        }
    }

    var connectedReceiverDisplayName: String {
        readCString(maxLength: 128) { buffer, length in
            picoo_sender_connected_receiver_display_name(sender, buffer, length)
        }
    }

    func connect(to endpoint: ReceiverEndpoint) throws {
        let connectCode = endpoint.host.withCString { host in
            picoo_sender_connect(sender, host, endpoint.port)
        }
        try check(connectCode, operation: "sender_connect")

        let publicKey = identity.publicKey
        let helloCode = identity.deviceID.withCString { deviceID in
            identity.deviceName.withCString { deviceName in
                publicKey.withUnsafeBytes { bytes in
                    picoo_sender_send_client_hello(
                        sender,
                        deviceID,
                        deviceName,
                        bytes.bindMemory(to: UInt8.self).baseAddress,
                        UInt(bytes.count)
                    )
                }
            }
        }
        try check(helloCode, operation: "sender_send_client_hello")
    }

    func confirmPairing(receiverID: String) throws {
        let code = receiverID.withCString { receiverID in
            picoo_sender_send_pairing_confirm(sender, receiverID)
        }
        try check(code, operation: "sender_send_pairing_confirm")
    }

    func pump() throws {
        try check(picoo_sender_pump(sender), operation: "sender_pump")
    }

    func disconnect() throws {
        try check(picoo_sender_disconnect(sender), operation: "sender_disconnect")
    }

    func markCameraPermissionRequired() throws {
        try check(
            picoo_sender_mark_permission_required(sender),
            operation: "sender_mark_permission_required"
        )
    }

    func clearCameraPermissionRequired() throws {
        try check(
            picoo_sender_clear_permission_required(sender),
            operation: "sender_clear_permission_required"
        )
    }

    func trustedReceiverIDs() -> Set<String> {
        guard let store = trustedStorePath.withCString({ picoo_trusted_store_load($0) }) else {
            return []
        }
        defer { picoo_trusted_store_destroy(store) }

        let count = picoo_trusted_store_count(store)
        return Set((0..<count).compactMap { index in
            var item = PicooTrustedDevice()
            guard picoo_trusted_store_get(store, index, &item) == 0 else { return nil }
            return stringFromFixedBytes(&item.device_id)
        })
    }

    private func check(_ code: Int32, operation: String) throws {
        guard code >= 0 else {
            throw PicooSenderSessionError.operationFailed(name: operation, code: code)
        }
    }

    private static func storageDirectory() throws -> URL {
        guard let applicationSupport = FileManager.default.urls(
            for: .applicationSupportDirectory,
            in: .userDomainMask
        ).first else {
            throw PicooSenderSessionError.storageUnavailable
        }
        let directory = applicationSupport.appendingPathComponent("PicooCamera", isDirectory: true)
        try FileManager.default.createDirectory(
            at: directory,
            withIntermediateDirectories: true
        )
        return directory
    }
}

final class PicooDiscoveryBrowser {
    private let browser: UnsafeMutableRawPointer

    init() throws {
        guard let browser = picoo_discovery_browser_create() else {
            throw PicooSenderSessionError.discoveryCreationFailed
        }
        self.browser = browser
    }

    isolated deinit {
        picoo_discovery_browser_destroy(browser)
    }

    func poll(trustedReceiverIDs: Set<String>) throws -> [ReceiverSummary] {
        let result = picoo_discovery_browser_poll(browser, 0)
        guard result >= 0 else {
            throw PicooSenderSessionError.operationFailed(
                name: "discovery_browser_poll",
                code: result
            )
        }

        return (0..<picoo_discovery_browser_count(browser)).compactMap { index in
            var item = PicooDiscoveredReceiver()
            guard picoo_discovery_browser_get(browser, index, &item) == 0 else { return nil }

            let receiverID = stringFromFixedBytes(&item.receiver_id)
            let displayName = stringFromFixedBytes(&item.display_name)
            let host = stringFromFixedBytes(&item.host)
            let pairingState = stringFromFixedBytes(&item.pairing_state)
            guard !receiverID.isEmpty, !displayName.isEmpty, !host.isEmpty else { return nil }

            return ReceiverSummary(
                id: receiverID,
                displayName: displayName,
                endpoint: ReceiverEndpoint(host: host, port: item.quic_port),
                pairingState: pairingState.isEmpty ? nil : pairingState,
                isTrusted: trustedReceiverIDs.contains(receiverID)
            )
        }
        .sorted { lhs, rhs in
            if lhs.isTrusted != rhs.isTrusted { return lhs.isTrusted }
            return lhs.displayName.localizedCaseInsensitiveCompare(rhs.displayName) == .orderedAscending
        }
    }
}

private final class PicooSenderIdentity {
    private let identity: UnsafeMutableRawPointer

    init(path: String, defaultDeviceName: String) throws {
        let handle = path.withCString { path in
            defaultDeviceName.withCString { name in
                picoo_identity_load_or_create(path, name)
            }
        }
        guard let handle else {
            throw PicooSenderSessionError.identityCreationFailed
        }
        identity = handle
    }

    isolated deinit {
        picoo_identity_destroy(identity)
    }

    var deviceID: String {
        readCString(maxLength: 128) { buffer, length in
            picoo_identity_device_id(identity, buffer, length)
        }
    }

    var deviceName: String {
        readCString(maxLength: 128) { buffer, length in
            picoo_identity_device_name(identity, buffer, length)
        }
    }

    var publicKey: Data {
        var bytes = [UInt8](repeating: 0, count: 128)
        let length = bytes.withUnsafeMutableBufferPointer { buffer in
            picoo_identity_public_key(identity, buffer.baseAddress, UInt(buffer.count))
        }
        guard length > 0 else { return Data() }
        return Data(bytes.prefix(Int(length)))
    }
}

private func readCString(
    maxLength: Int,
    _ body: (UnsafeMutablePointer<CChar>?, UInt) -> Int32
) -> String {
    var buffer = [CChar](repeating: 0, count: maxLength)
    let length = buffer.withUnsafeMutableBufferPointer { pointer in
        body(pointer.baseAddress, UInt(pointer.count))
    }
    guard length > 0 else { return "" }
    return String(decoding: buffer.prefix(Int(length)).map(UInt8.init(bitPattern:)), as: UTF8.self)
}

private func stringFromFixedBytes<Value>(_ value: inout Value) -> String {
    withUnsafeBytes(of: &value) { rawBuffer in
        let bytes = rawBuffer.prefix { $0 != 0 }
        return String(decoding: bytes, as: UTF8.self)
    }
}
