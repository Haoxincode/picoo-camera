import SwiftUI
import UIKit

@main
struct PicooCameraApp: App {
    @State private var model: SenderAppModel

    init() {
        let session = try? PicooSenderSession(defaultDeviceName: UIDevice.current.name)
        _model = State(initialValue: SenderAppModel(session: session))
    }

    var body: some Scene {
        WindowGroup {
            ContentView(model: model)
        }
    }
}
