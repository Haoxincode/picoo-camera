import Foundation

// Preparation candidates are not proof of local Camera/Encoder support — MEDIA-054.
nonisolated struct VideoSourceFormat: Equatable, Sendable {
    let codec: NativeVideoCodec
    let resolution: VideoResolution
    let framesPerSecond: UInt32

    init?(codec: UInt32, height: UInt32, framesPerSecond: UInt32) {
        guard let codec = NativeVideoCodec(rawValue: codec),
              let resolution = VideoResolution.supported(forRequestedHeight: height),
              framesPerSecond == 30 || framesPerSecond == 60 else { return nil }
        self.codec = codec
        self.resolution = resolution
        self.framesPerSecond = framesPerSecond
    }
}
