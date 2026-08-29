import XCTest
@testable import PicooCamera

final class PicooSenderSessionTests: XCTestCase {
    func testProtocolVersionCrossesSwiftCAbiBoundary() {
        XCTAssertEqual(PicooSenderSession.protocolVersion, "PCP/1")
    }

    func testSenderHandleStartsDisconnectedAndOwnerReleases() throws {
        weak var releasedSession: PicooSenderSession?

        try autoreleasepool {
            let session = try PicooSenderSession()
            releasedSession = session
            XCTAssertEqual(session.statusCode, 0)
            try session.disconnect()
            XCTAssertEqual(session.statusCode, 0)
        }

        XCTAssertNil(releasedSession)
    }
}
