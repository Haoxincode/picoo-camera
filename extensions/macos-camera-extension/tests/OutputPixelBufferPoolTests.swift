import CoreMedia
import CoreVideo
import Foundation

// Runs the production pool with real CoreVideo allocations and CMSampleBuffer retention.
func verifyOutputPixelBufferPool() throws {
    let pool = try OutputPixelBufferPool(width: 1280, height: 720)
    var samples: [CMSampleBuffer] = []
    for _ in 0..<3 {
        let sample: CMSampleBuffer = try autoreleasepool {
            guard let image = try pool.acquire() else { throw CocoaError(.coderInvalidValue) }
            var description: CMVideoFormatDescription?
            guard CMVideoFormatDescriptionCreateForImageBuffer(
                allocator: kCFAllocatorDefault, imageBuffer: image,
                formatDescriptionOut: &description
            ) == noErr, let description else { throw CocoaError(.coderInvalidValue) }
            var timing = CMSampleTimingInfo(
                duration: CMTime(value: 1, timescale: 30),
                presentationTimeStamp: .zero, decodeTimeStamp: .invalid
            )
            var sample: CMSampleBuffer?
            guard CMSampleBufferCreateForImageBuffer(
                allocator: kCFAllocatorDefault, imageBuffer: image, dataReady: true,
                makeDataReadyCallback: nil, refcon: nil, formatDescription: description,
                sampleTiming: &timing, sampleBufferOut: &sample
            ) == noErr, let sample else { throw CocoaError(.coderInvalidValue) }
            return sample
        }
        samples.append(sample)
    }
    guard try pool.acquire() == nil else { throw CocoaError(.coderInvalidValue) }
    // Only the system sample owns the image now; CPU local-reference release did not free it.
    samples.removeLast()
    try autoreleasepool {
        guard let reclaimed = try pool.acquire() else { throw CocoaError(.coderInvalidValue) }
        guard CVPixelBufferGetWidth(reclaimed) == 1280,
              try pool.acquire() == nil else { throw CocoaError(.coderInvalidValue) }
        withExtendedLifetime(reclaimed) {}
    }
    samples.removeAll()
    guard try pool.acquire() != nil else { throw CocoaError(.coderInvalidValue) }
    print("CoreVideo output pool: three retained samples bound allocation; release recovers")
}
