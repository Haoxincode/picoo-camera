import CoreMediaIO
import Foundation
import os

let clientQueue = DispatchQueue(
    label: "com.haoxincode.picoo-camera.extension.clients",
    qos: .userInitiated,
    autoreleaseFrequency: .workItem
)

do {
    let providerSource = try PicooCameraProviderSource(clientQueue: clientQueue)
    CMIOExtensionProvider.startService(provider: providerSource.provider)
    CFRunLoopRun()
} catch {
    Logger(
        subsystem: "com.haoxincode.picoo-camera.camera-extension",
        category: "CameraExtension"
    ).fault("Unable to start Picoo Camera Extension: \(error.localizedDescription, privacy: .public)")
    exit(EXIT_FAILURE)
}
