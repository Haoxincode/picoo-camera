// Build with the production iOS VideoEncoder{,Output,Pipeline}.swift sources.
// Synthetic input on macOS validates shared Apple APIs, not iPhone camera throughput.
import AVFoundation
import CoreMedia
import CoreVideo
import Foundation

nonisolated final class Events: @unchecked Sendable {
    private let lock = NSLock()
    private var values: [VideoEncoderEvent] = []
    func receive(_ event: VideoEncoderEvent) { lock.withLock { values.append(event) } }
    func take() -> [VideoEncoderEvent] { lock.withLock { values } }
}

nonisolated final class Input: @unchecked Sendable {
    let sample: CMSampleBuffer
    init(width: Int, height: Int, fps: Int32, describeColor: Bool = true) throws {
        var image: CVPixelBuffer?
        let attributes = [kCVPixelBufferIOSurfacePropertiesKey: [:]] as CFDictionary
        guard CVPixelBufferCreate(nil, width, height, kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
                                  attributes, &image) == kCVReturnSuccess, let image else {
            throw Failure.message("pixel allocation failed")
        }
        // Diagnostic initialization only. Production capture owns native camera pixels.
        CVPixelBufferLockBaseAddress(image, [])
        for plane in 0..<2 {
            memset(CVPixelBufferGetBaseAddressOfPlane(image, plane), 128,
                   CVPixelBufferGetBytesPerRowOfPlane(image, plane) * CVPixelBufferGetHeightOfPlane(image, plane))
        }
        CVPixelBufferUnlockBaseAddress(image, [])
        for (key, value) in [
            (kCVImageBufferColorPrimariesKey, kCVImageBufferColorPrimaries_ITU_R_709_2),
            (kCVImageBufferTransferFunctionKey, kCVImageBufferTransferFunction_ITU_R_709_2),
            (kCVImageBufferYCbCrMatrixKey, kCVImageBufferYCbCrMatrix_ITU_R_709_2),
        ] { if describeColor { CVBufferSetAttachment(image, key, value, .shouldPropagate) } }
        var format: CMVideoFormatDescription?
        guard CMVideoFormatDescriptionCreateForImageBuffer(allocator: nil, imageBuffer: image,
                                                           formatDescriptionOut: &format) == noErr,
              let format else { throw Failure.message("input format failed") }
        var timing = CMSampleTimingInfo(duration: CMTime(value: 1, timescale: fps),
                                       presentationTimeStamp: CMTime(value: 1, timescale: fps),
                                       decodeTimeStamp: .invalid)
        var sample: CMSampleBuffer?
        guard CMSampleBufferCreateReadyWithImageBuffer(allocator: nil, imageBuffer: image,
            formatDescription: format, sampleTiming: &timing, sampleBufferOut: &sample) == noErr,
              let sample else { throw Failure.message("input sample failed") }
        self.sample = sample
    }
}

nonisolated enum Failure: Error { case message(String) }

@main struct EncoderHarness {
    static func main() async throws {
        let missingColor = Events()
        let request = VideoEncoderConfiguration(codec: .hevc, resolution: .p720,
            framesPerSecond: 30, bitrateBps: 3_000_000, streamEpoch: 1, encoderGeneration: 1)
        let rejected = VideoEncoderPipeline(initialConfiguration: request, eventHandler: missingColor.receive)
        let undescribed = try Input(width: 1280, height: 720, fps: 30, describeColor: false)
        await rejected.start(configuration: request)
        await withCheckedContinuation { continuation in
            rejected.callbackQueue.async { rejected.submit(undescribed.sample); continuation.resume() }
        }
        await rejected.stop()
        let failures = missingColor.take()
        guard failures.count == 1, case .failure = failures[0] else {
            throw Failure.message("unknown color was not rejected")
        }
        print("PASS unknown source color rejected")
        for (width, height) in [(640, 360), (1920, 1080), (720, 1280)] {
            let events = Events()
            let config = VideoEncoderConfiguration(codec: .avc, resolution: .p720,
                framesPerSecond: 30, bitrateBps: 3_000_000, streamEpoch: 3, encoderGeneration: 4)
            let encoder = VideoEncoderPipeline(initialConfiguration: config, eventHandler: events.receive)
            let input = try Input(width: width, height: height, fps: 30)
            await encoder.start(configuration: config)
            await withCheckedContinuation { continuation in
                encoder.callbackQueue.async {
                    encoder.submit(input.sample)
                    continuation.resume()
                }
            }
            await encoder.stop()
            let output = events.take()
            guard output.count == 1, case let .failure(epoch, generation, message) = output[0],
                  epoch == 3, generation == 4,
                  message == VideoEncoderError.sourceDimensionsMismatch.localizedDescription else {
                throw Failure.message("incorrect source dimensions were not rejected")
            }
        }
        print("PASS smaller, larger, and transposed capture dimensions rejected")
        for codec in NativeVideoCodec.allCases {
            for resolution in VideoResolution.allCases {
                for fps: UInt32 in [30, 60] {
                    let events = Events()
                    let config = VideoEncoderConfiguration(codec: codec, resolution: resolution,
                        framesPerSecond: fps, bitrateBps: 6_000_000, streamEpoch: 7,
                        encoderGeneration: 11, rotation: 0)
                    let encoder = VideoEncoderPipeline(initialConfiguration: config, eventHandler: events.receive)
                    let input = try Input(width: Int(resolution.width), height: Int(resolution.height), fps: Int32(fps))
                    await encoder.start(configuration: config)
                    await withCheckedContinuation { continuation in
                        encoder.callbackQueue.async {
                            encoder.submit(input.sample)
                            continuation.resume()
                        }
                    }
                    await encoder.stop()
                    let output = events.take()
                    guard output.count == 1, case let .accessUnit(frame) = output[0],
                          frame.isKeyframe, frame.codecConfiguration.codec == codec.rawValue,
                          !frame.codecConfiguration.record.isEmpty, !frame.data.isEmpty,
                          frame.width == UInt32(resolution.width), frame.height == UInt32(resolution.height),
                          frame.framesPerSecond == fps, frame.rotation == 0,
                          frame.encoderGeneration == 11, frame.streamEpoch == 7 else {
                        throw Failure.message("unexpected \(codec) \(resolution) \(fps) output: \(output)")
                    }
                    if CommandLine.arguments.count == 2 {
                        let directory = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
                        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
                        let stem = "\(codec.rawValue)-\(resolution.rawValue)-\(fps)"
                        try frame.data.write(to: directory.appendingPathComponent(stem + ".au"))
                        try frame.codecConfiguration.record.write(to: directory.appendingPathComponent(stem + ".config"))
                    }
                    // A direction change rebuilds the native generation. Each AU
                    // retains the configuration captured by its own VT callback.
                    for (rotation, epoch, generation) in [(UInt32(90), UInt32(8), UInt64(12)), (0, 9, 13)] {
                        let next = VideoEncoderConfiguration(codec: codec, resolution: resolution,
                            framesPerSecond: fps, bitrateBps: 6_000_000, streamEpoch: epoch,
                            encoderGeneration: generation, rotation: rotation)
                        await encoder.start(configuration: next)
                        await withCheckedContinuation { continuation in
                            encoder.callbackQueue.async {
                                encoder.submit(input.sample)
                                continuation.resume()
                            }
                        }
                        await encoder.stop()
                        guard case let .accessUnit(rotated)? = events.take().last,
                              rotated.rotation == rotation, rotated.streamEpoch == epoch,
                              rotated.encoderGeneration == generation, rotated.isKeyframe,
                              rotated.width == frame.width, rotated.height == frame.height,
                              rotated.framesPerSecond == fps else {
                            throw Failure.message("direction generation did not match")
                        }
                    }
                    print("PASS \(codec) \(resolution.rawValue)p\(fps) AU=\(frame.data.count) record=\(frame.codecConfiguration.record.count)")
                }
            }
        }
    }
}
