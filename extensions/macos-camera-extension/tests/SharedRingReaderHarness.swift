import CoreVideo
import Darwin
import Foundation

private enum HarnessError: Error, CustomStringConvertible {
    case usage
    case noFrame
    case invalidFrame(String)
    case timeout(observed: Int, lastTimestamp: UInt64)

    var description: String {
        switch self {
        case .usage:
            return "usage: picoo-shared-ring-reader-harness <read-once|stress|leak-and-exit> ..."
        case .noFrame:
            return "no complete frame was available"
        case let .invalidFrame(message):
            return "invalid frame: \(message)"
        case let .timeout(observed, lastTimestamp):
            return "timed out after \(observed) unique frames; last timestamp=\(lastTimestamp)"
        }
    }
}

@main
private enum SharedRingReaderHarness {
    static func main() {
        do {
            try run(arguments: Array(CommandLine.arguments.dropFirst()))
        } catch {
            fputs("SharedRingReaderHarness: \(error)\n", stderr)
            Darwin.exit(EXIT_FAILURE)
        }
    }

    private static func run(arguments: [String]) throws {
        guard let mode = arguments.first else {
            throw HarnessError.usage
        }
        switch mode {
        case "read-once":
            guard arguments.count == 3, let timestamp = UInt64(arguments[2]) else {
                throw HarnessError.usage
            }
            let reader = try SharedRingReader(fileURL: URL(fileURLWithPath: arguments[1]))
            guard let frame = reader.acquireLatestFrame() else {
                throw HarnessError.noFrame
            }
            try validate(frame: frame, expectedTimestamp: timestamp)

        case "stress":
            guard arguments.count == 5,
                  let finalTimestamp = UInt64(arguments[3]),
                  let minimumUniqueFrames = Int(arguments[4]),
                  minimumUniqueFrames > 0
            else {
                throw HarnessError.usage
            }
            try stress(
                ringPath: arguments[1],
                readyPath: arguments[2],
                finalTimestamp: finalTimestamp,
                minimumUniqueFrames: minimumUniqueFrames
            )

        case "leak-and-exit":
            guard arguments.count == 3, let timestamp = UInt64(arguments[2]) else {
                throw HarnessError.usage
            }
            let reader = try SharedRingReader(fileURL: URL(fileURLWithPath: arguments[1]))
            guard let frame = reader.acquireLatestFrame() else {
                throw HarnessError.noFrame
            }
            try validate(frame: frame, expectedTimestamp: timestamp)
            // Model abrupt Camera Extension termination. Retaining the frame
            // ensures ARC cannot release the atomic lease before `_exit`;
            // Darwin still closes the slot's flock descriptor for recovery.
            _ = Unmanaged.passRetained(frame)
            Darwin._exit(EXIT_SUCCESS)

        default:
            throw HarnessError.usage
        }
    }

    private static func stress(
        ringPath: String,
        readyPath: String,
        finalTimestamp: UInt64,
        minimumUniqueFrames: Int
    ) throws {
        let reader = try SharedRingReader(fileURL: URL(fileURLWithPath: ringPath))
        try Data().write(to: URL(fileURLWithPath: readyPath), options: .atomic)

        var observedSequences = Set<UInt64>()
        var lastTimestamp: UInt64 = 0
        let deadline = ProcessInfo.processInfo.systemUptime + 8
        while ProcessInfo.processInfo.systemUptime < deadline {
            guard let frame = reader.acquireLatestFrame() else {
                usleep(100)
                continue
            }
            try validate(frame: frame, expectedTimestamp: frame.timestampMicroseconds)
            guard observedSequences.insert(frame.sequence).inserted else {
                usleep(100)
                continue
            }
            lastTimestamp = frame.timestampMicroseconds
            if lastTimestamp == finalTimestamp,
               observedSequences.count >= minimumUniqueFrames {
                return
            }
        }
        throw HarnessError.timeout(
            observed: observedSequences.count,
            lastTimestamp: lastTimestamp
        )
    }

    private static func validate(
        frame: SharedRingFrame,
        expectedTimestamp: UInt64
    ) throws {
        guard frame.timestampMicroseconds == expectedTimestamp else {
            throw HarnessError.invalidFrame(
                "timestamp \(frame.timestampMicroseconds) != \(expectedTimestamp)"
            )
        }
        guard frame.width == 64,
              frame.height == 64,
              frame.stride == 80,
              frame.rotation == 0
        else {
            throw HarnessError.invalidFrame(
                "metadata \(frame.width)x\(frame.height) stride=\(frame.stride) rotation=\(frame.rotation)"
            )
        }

        var pixelBuffer: CVPixelBuffer?
        let attributes: NSDictionary = [
            kCVPixelBufferIOSurfacePropertiesKey: [:] as NSDictionary,
        ]
        guard CVPixelBufferCreate(
            kCFAllocatorDefault,
            frame.width,
            frame.height,
            kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
            attributes,
            &pixelBuffer
        ) == kCVReturnSuccess, let pixelBuffer else {
            throw HarnessError.invalidFrame("unable to allocate CVPixelBuffer")
        }
        guard frame.copyNV12(to: pixelBuffer, workspace: VImageScaleWorkspace()) else {
            throw HarnessError.invalidFrame("NV12 copy failed")
        }

        guard CVPixelBufferLockBaseAddress(pixelBuffer, .readOnly) == kCVReturnSuccess else {
            throw HarnessError.invalidFrame("unable to lock CVPixelBuffer")
        }
        defer { CVPixelBufferUnlockBaseAddress(pixelBuffer, .readOnly) }

        for plane in 0..<CVPixelBufferGetPlaneCount(pixelBuffer) {
            guard let base = CVPixelBufferGetBaseAddressOfPlane(pixelBuffer, plane) else {
                throw HarnessError.invalidFrame("plane \(plane) has no base address")
            }
            let bytesPerRow = CVPixelBufferGetBytesPerRowOfPlane(pixelBuffer, plane)
            let height = CVPixelBufferGetHeightOfPlane(pixelBuffer, plane)
            // NV12's UV plane reports chroma samples, while each sample is two
            // interleaved bytes. SharedRingFrame copies `frame.width` active
            // bytes for both planes, so validate that complete byte range.
            let activeBytesPerRow = frame.width
            for row in 0..<height {
                let bytes = base.advanced(by: row * bytesPerRow)
                    .assumingMemoryBound(to: UInt8.self)
                for column in 0..<activeBytesPerRow {
                    let planeOffset = plane == 0 ? 0 : frame.stride * frame.height
                    let sourceOffset = planeOffset + row * frame.stride + column
                    let expected = UInt8(
                        (
                            expectedTimestamp
                                + UInt64(sourceOffset) * 31
                                + UInt64(sourceOffset / 256) * 17
                        ) % 251
                    )
                    guard bytes[column] == expected else {
                        throw HarnessError.invalidFrame(
                            "torn pixel at plane=\(plane) row=\(row) column=\(column): "
                                + "\(bytes[column]) != \(expected)"
                        )
                    }
                }
            }
        }
    }
}
