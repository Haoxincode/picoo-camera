import SwiftUI

@main
struct PicooCameraApp: App {
    private let senderSession: PicooSenderSession?

    init() {
        senderSession = try? PicooSenderSession()
    }

    var body: some Scene {
        WindowGroup {
            ContentView(
                protocolVersion: PicooSenderSession.protocolVersion,
                coreReady: senderSession != nil
            )
        }
    }
}
