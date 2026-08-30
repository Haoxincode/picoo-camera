import Foundation
import PicooCore

nonisolated enum PicooSenderSessionError: Error, Equatable {
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
nonisolated struct SenderStreamConfiguration: Equatable, Sendable {
    let width: UInt32
    let height: UInt32
    let framesPerSecond: UInt32
    let bitrateBps: UInt32
    let streamEpoch: UInt32
    let mirrored: Bool
    let rotation: UInt32
    let sequenceParameterSet: Data
    let pictureParameterSet: Data
}

nonisolated enum SenderCameraCommand: Equatable, Sendable {
    case switchFront
    case switchBack
    case setResolution(width: UInt32, height: UInt32)
    case setMirror(Bool)
}

nonisolated struct SenderEncoderDirective: Equatable, Sendable {
    let id: UInt64
    let kind: UInt32
    let targetHeight: UInt32
    let targetBitrateBps: UInt32
    let streamEpoch: UInt32
}

nonisolated struct SenderSessionSnapshot: Equatable, Sendable {
    let status: PicooSenderStatus
    let currentBitrateBps: UInt32
    let activeHeight: UInt32
    let receiverMaxHeight: UInt32
    let streamEpoch: UInt32
    let reconnectAttempt: UInt32
    let reconnectDelayMs: UInt64
}

/// Rust's `SenderInner` serializes every operation with one `Mutex`. The Swift
/// wrapper therefore supports the MainActor control plane and the media actor
/// calling the same handle concurrently. Both retain this object for its full
/// use, so `deinit` cannot race an in-flight FFI call.
nonisolated final class PicooSenderSession: @unchecked Sendable {
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

    deinit {
        picoo_sender_destroy(sender)
    }

    var snapshot: SenderSessionSnapshot {
        var value = PicooSenderSnapshot()
        guard picoo_sender_snapshot(sender, &value) == 0 else {
            return SenderSessionSnapshot(
                status: .disconnected,
                currentBitrateBps: 0,
                activeHeight: 0,
                receiverMaxHeight: 0,
                streamEpoch: Self.initialStreamEpoch,
                reconnectAttempt: 0,
                reconnectDelayMs: 0
            )
        }
        return SenderSessionSnapshot(
            status: PicooSenderStatus(code: value.status),
            currentBitrateBps: value.current_bitrate_bps,
            activeHeight: value.active_height,
            receiverMaxHeight: value.receiver_max_height,
            streamEpoch: value.stream_epoch,
            reconnectAttempt: value.reconnect_attempt,
            reconnectDelayMs: value.reconnect_delay_ms
        )
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

    func setStreamConfiguration(_ configuration: SenderStreamConfiguration) throws {
        let code = configuration.sequenceParameterSet.withUnsafeBytes { sequenceBytes in
            configuration.pictureParameterSet.withUnsafeBytes { pictureBytes in
                picoo_sender_set_stream_config(
                    sender,
                    configuration.width,
                    configuration.height,
                    configuration.framesPerSecond,
                    configuration.bitrateBps,
                    configuration.mirrored ? 1 : 0,
                    configuration.rotation,
                    sequenceBytes.bindMemory(to: UInt8.self).baseAddress,
                    UInt(sequenceBytes.count),
                    pictureBytes.bindMemory(to: UInt8.self).baseAddress,
                    UInt(pictureBytes.count)
                )
            }
        }
        try check(code, operation: "sender_set_stream_config")
    }

    func send(_ accessUnit: EncodedAccessUnit) throws {
        var packetCount: UInt32 = 0
        let ingestCode = accessUnit.data.withUnsafeBytes { bytes in
            picoo_sender_ingest_access_unit(
                sender,
                bytes.bindMemory(to: UInt8.self).baseAddress,
                UInt(bytes.count),
                accessUnit.isKeyframe ? 1 : 0,
                accessUnit.presentationTimeUs,
                accessUnit.streamEpoch,
                &packetCount
            )
        }
        try check(ingestCode, operation: "sender_ingest_access_unit")
        guard packetCount > 0 else { return }

        var sentCount: UInt32 = 0
        try check(
            picoo_sender_flush(sender, &sentCount),
            operation: "sender_flush"
        )
        try pump()
    }

    func takeKeyframeRequest() throws -> Bool {
        try takeFlag(
            picoo_sender_take_keyframe_request(sender),
            operation: "sender_take_keyframe_request"
        )
    }

    func encoderDirective() throws -> SenderEncoderDirective? {
        var directive = PicooEncoderDirective()
        let code = picoo_sender_peek_encoder_directive(sender, &directive)
        try check(code, operation: "sender_peek_encoder_directive")
        guard code == 1 else { return nil }
        return SenderEncoderDirective(
            id: directive.id,
            kind: directive.kind,
            targetHeight: directive.target_height,
            targetBitrateBps: directive.target_bitrate_bps,
            streamEpoch: directive.stream_epoch
        )
    }

    func acknowledgeEncoderDirective(_ id: UInt64, actualHeight: UInt32) throws {
        guard picoo_sender_ack_encoder_directive(sender, id, actualHeight) == 1 else {
            throw PicooSenderSessionError.operationFailed(
                name: "sender_ack_encoder_directive",
                code: 0
            )
        }
    }

    func rejectEncoderDirective(_ id: UInt64) throws {
        guard picoo_sender_nack_encoder_directive(sender, id) == 1 else {
            throw PicooSenderSessionError.operationFailed(
                name: "sender_nack_encoder_directive",
                code: 0
            )
        }
    }

    func takeCameraCommand() throws -> SenderCameraCommand? {
        var width: UInt32 = 0
        var height: UInt32 = 0
        var mirrored: Int32 = 0
        let command = picoo_sender_take_camera_command(
            sender,
            &width,
            &height,
            &mirrored
        )
        switch command {
        case 0:
            return nil
        case 1:
            return .switchFront
        case 2:
            return .switchBack
        case 3:
            return .setResolution(width: width, height: height)
        case 4:
            return .setMirror(mirrored != 0)
        default:
            throw PicooSenderSessionError.operationFailed(
                name: "sender_take_camera_command",
                code: command
            )
        }
    }

    func setPreferredHeight(_ height: UInt32) throws {
        try check(
            picoo_sender_set_preferred_height(sender, height),
            operation: "sender_set_preferred_height"
        )
    }

    func beginStreamReconfiguration() -> UInt32 {
        picoo_sender_begin_stream_reconfiguration(sender)
    }

    func cancelStreamReconfiguration(_ streamEpoch: UInt32) throws {
        try check(
            picoo_sender_cancel_stream_reconfiguration(sender, streamEpoch),
            operation: "sender_cancel_stream_reconfiguration"
        )
    }

    func reportEncoderHeight(_ height: UInt32, streamEpoch: UInt32) throws {
        try check(
            picoo_sender_report_encoder_height(sender, height, streamEpoch),
            operation: "sender_report_encoder_height"
        )
    }

    static func initialBitrate(forHeight height: UInt32) -> UInt32 {
        picoo_bitrate_initial_for_height(height)
    }

    static func clampBitrate(_ bitrate: UInt32, forHeight height: UInt32) -> UInt32 {
        picoo_bitrate_clamp_for_height(bitrate, height)
    }

    static var initialStreamEpoch: UInt32 {
        picoo_stream_epoch_initial()
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

    private func takeFlag(_ code: Int32, operation: String) throws -> Bool {
        guard code >= 0 else {
            throw PicooSenderSessionError.operationFailed(name: operation, code: code)
        }
        return code == 1
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

nonisolated private final class PicooSenderIdentity {
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

    deinit {
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

nonisolated private func readCString(
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

nonisolated private func stringFromFixedBytes<Value>(_ value: inout Value) -> String {
    withUnsafeBytes(of: &value) { rawBuffer in
        let bytes = rawBuffer.prefix { $0 != 0 }
        return String(decoding: bytes, as: UTF8.self)
    }
}
