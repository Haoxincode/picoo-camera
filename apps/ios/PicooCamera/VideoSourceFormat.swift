import Foundation

// Preparation candidates are not proof of local Camera/Encoder support — MEDIA-054.
nonisolated struct VideoSourceFormat: Hashable, Sendable {
    let codec: NativeVideoCodec
    let resolution: VideoResolution
    let framesPerSecond: UInt32

    static let defaultFormat = Self(codec: .avc, resolution: .p1080, framesPerSecond: 60)
    static let productFormats = NativeVideoCodec.allCases.flatMap { codec in
        VideoResolution.allCases.flatMap { resolution in
            [UInt32(60), 30].map { Self(codec: codec, resolution: resolution, framesPerSecond: $0) }
        }
    }

    var label: String {
        "\(codec == .avc ? "H.264" : "HEVC") · \(resolution.rawValue)p · \(framesPerSecond) fps"
    }

    init(codec: NativeVideoCodec, resolution: VideoResolution, framesPerSecond: UInt32) {
        precondition(framesPerSecond == 30 || framesPerSecond == 60)
        self.codec = codec
        self.resolution = resolution
        self.framesPerSecond = framesPerSecond
    }

    func matches(_ accessUnit: EncodedAccessUnit) -> Bool {
        accessUnit.codecConfiguration.codec == codec.rawValue
            && accessUnit.width == UInt32(resolution.width)
            && accessUnit.height == UInt32(resolution.height)
            && accessUnit.framesPerSecond == framesPerSecond
    }

    init?(codec: UInt32, height: UInt32, framesPerSecond: UInt32) {
        guard let codec = NativeVideoCodec(rawValue: codec),
              let resolution = VideoResolution.supported(forRequestedHeight: height),
              framesPerSecond == 30 || framesPerSecond == 60 else { return nil }
        self.codec = codec
        self.resolution = resolution
        self.framesPerSecond = framesPerSecond
    }
}
