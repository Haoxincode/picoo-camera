import Foundation
import PicooCore

enum PicooSenderSessionError: Error, Equatable {
    case senderCreationFailed
    case operationFailed(name: String, code: Int32)
}

/// Swift-owned lifetime boundary for the opaque Rust Sender handle.
///
/// Raw pointers never escape this type. Platform camera code will hand encoded
/// H.264 access units to this session in REQ-PICOO-MEDIA-011; capture buffers do
/// not cross the FFI boundary.
final class PicooSenderSession {
    private let sender: UnsafeMutableRawPointer

    static var protocolVersion: String {
        guard let version = picoo_protocol_version() else {
            return "Unavailable"
        }
        return String(cString: version)
    }

    init() throws {
        guard let sender = picoo_sender_create() else {
            throw PicooSenderSessionError.senderCreationFailed
        }
        self.sender = sender
    }

    deinit {
        picoo_sender_destroy(sender)
    }

    var statusCode: Int32 {
        return picoo_sender_status(sender)
    }

    func pump() throws {
        try check(picoo_sender_pump(sender), operation: "sender_pump")
    }

    func disconnect() throws {
        try check(picoo_sender_disconnect(sender), operation: "sender_disconnect")
    }

    private func check(_ code: Int32, operation: String) throws {
        guard code >= 0 else {
            throw PicooSenderSessionError.operationFailed(name: operation, code: code)
        }
    }
}
