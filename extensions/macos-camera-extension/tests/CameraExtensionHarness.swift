import Darwin
import Foundation

@main
private enum CameraExtensionHarness {
    static func main() {
        do {
            guard let mode = CommandLine.arguments.dropFirst().first else {
                throw CocoaError(.coderInvalidValue)
            }
            switch mode {
            case "output-clock":
                try verifyOutputSampleClock()
                try verifyOutputFormatProperties()
            case "output-pool":
                try verifyOutputPixelBufferPool()
            default:
                throw CocoaError(.coderInvalidValue)
            }
        } catch {
            fputs("CameraExtensionHarness: \(error)\n", stderr)
            Darwin.exit(EXIT_FAILURE)
        }
    }
}
