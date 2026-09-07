// REQ-PICOO-NEXT-018/022. Synthetic AU fixtures only. Tests the platform passthrough mux contract,
// not recording queue policy, durable finalization, or real camera throughput.
import AVFoundation
import CoreMedia
import Foundation
import Darwin

enum ProbeFailure: Error { case message(String) }

func checked(_ status: OSStatus, _ operation: String) throws {
    guard status == noErr else { throw ProbeFailure.message("\(operation): \(status)") }
}

func sample(data: Data, format: CMFormatDescription, index: Int64, fps: Int32) throws -> CMSampleBuffer {
    var block: CMBlockBuffer?
    try checked(CMBlockBufferCreateWithMemoryBlock(allocator: nil, memoryBlock: nil,
        blockLength: data.count, blockAllocator: nil, customBlockSource: nil,
        offsetToData: 0, dataLength: data.count, flags: 0, blockBufferOut: &block), "block")
    guard let block else { throw ProbeFailure.message("missing block") }
    try data.withUnsafeBytes { bytes in
        try checked(CMBlockBufferReplaceDataBytes(with: bytes.baseAddress!, blockBuffer: block,
            offsetIntoDestination: 0, dataLength: data.count), "copy compressed AU")
    }
    var timing = CMSampleTimingInfo(duration: CMTime(value: 1, timescale: fps),
        presentationTimeStamp: CMTime(value: index, timescale: fps), decodeTimeStamp: .invalid)
    var size = data.count
    var result: CMSampleBuffer?
    try checked(CMSampleBufferCreateReady(allocator: nil, dataBuffer: block,
        formatDescription: format, sampleCount: 1, sampleTimingEntryCount: 1,
        sampleTimingArray: &timing, sampleSizeEntryCount: 1, sampleSizeArray: &size,
        sampleBufferOut: &result), "sample")
    guard let result else { throw ProbeFailure.message("missing sample") }
    return result
}

@main struct AppleMuxProbe {
    static func main() async throws {
        guard (3...4).contains(CommandLine.arguments.count) else {
            throw ProbeFailure.message("usage: apple-mux-probe INPUT_DIRECTORY OUTPUT_DIRECTORY [--interrupt-avc|--interrupt-hevc]")
        }
        let interruption = CommandLine.arguments.count == 4 ? CommandLine.arguments[3] : nil
        guard interruption == nil || ["--interrupt-avc", "--interrupt-hevc"].contains(interruption!) else {
            throw ProbeFailure.message("unknown interruption mode")
        }
        let interrupted = interruption != nil
        let source = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
        let destination = URL(fileURLWithPath: CommandLine.arguments[2], isDirectory: true)
        try FileManager.default.createDirectory(at: destination, withIntermediateDirectories: true)
        for codec in [1, 2] {
            if interruption == "--interrupt-hevc" && codec != 2 { continue }
            if interruption == "--interrupt-avc" && codec != 1 { continue }
            for height in [720, 1080] {
                for fps in [Int32(30), 60] {
                    let stem = "\(codec)-\(height)-\(fps)"
                    let data = try Data(contentsOf: source.appendingPathComponent(stem + ".au"))
                    let record = try Data(contentsOf: source.appendingPathComponent(stem + ".config"))
                    var format: CMFormatDescription?
                    let atoms = [codec == 1 ? "avcC" : "hvcC": record] as CFDictionary
                    let extensions = [kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms: atoms] as CFDictionary
                    try checked(CMVideoFormatDescriptionCreate(allocator: nil,
                        codecType: codec == 1 ? kCMVideoCodecType_H264 : kCMVideoCodecType_HEVC,
                        width: height == 720 ? 1280 : 1920, height: Int32(height),
                        extensions: extensions, formatDescriptionOut: &format), "format")
                    guard let format else { throw ProbeFailure.message("missing format") }
                    let url = destination.appendingPathComponent(stem + ".mp4")
                    // Existing destinations are rejected by AVAssetWriter; no overwrite.
                    let writer = try AVAssetWriter(outputURL: url, fileType: .mp4)
                    let input = AVAssetWriterInput(mediaType: .video, outputSettings: nil,
                        sourceFormatHint: format)
                    input.expectsMediaDataInRealTime = true
                    guard writer.canAdd(input) else { throw ProbeFailure.message("cannot add \(stem)") }
                    writer.add(input)
                    writer.movieFragmentInterval = CMTime(value: 1, timescale: 2)
                    guard writer.startWriting() else { throw writer.error ?? ProbeFailure.message("start") }
                    writer.startSession(atSourceTime: .zero)
                    for index in 0...Int64(fps * 3) {
                        let deadline = ContinuousClock.now.advanced(by: .seconds(2))
                        while !input.isReadyForMoreMediaData {
                            guard ContinuousClock.now < deadline else { throw ProbeFailure.message("writer stalled") }
                            try await Task.sleep(for: .milliseconds(1))
                        }
                        guard input.append(try sample(data: data, format: format, index: index, fps: fps)) else {
                            throw writer.error ?? ProbeFailure.message("append")
                        }
                    }
                    if interrupted {
                        // Deliberate process exit skips AVAssetWriter finalization.
                        // Only this probe process and synthetic partial file are affected.
                        try await Task.sleep(for: .seconds(1))
                        _exit(0)
                    }
                    input.markAsFinished()
                    await writer.finishWriting()
                    guard writer.status == .completed else { throw writer.error ?? ProbeFailure.message("finish") }
                    let asset = AVURLAsset(url: url)
                    let tracks = try await asset.loadTracks(withMediaType: .video)
                    guard let track = tracks.first else { throw ProbeFailure.message("missing track") }
                    let reader = try AVAssetReader(asset: asset)
                    let output = AVAssetReaderTrackOutput(track: track, outputSettings: nil)
                    reader.add(output)
                    guard reader.startReading() else { throw reader.error ?? ProbeFailure.message("read") }
                    var count: Int64 = 0
                    while let restored = output.copyNextSampleBuffer() {
                        // AVAssetReaderOutput.h permits marker-only buffers when
                        // outputSettings is nil. They are not compressed AUs.
                        if restored.numSamples == 0 { continue }
                        guard let block = restored.dataBuffer else { throw ProbeFailure.message("missing AU samples=\(restored.numSamples) valid=\(restored.isValid) pts=\(restored.presentationTimeStamp)" ) }
                        var bytes = Data(count: CMBlockBufferGetDataLength(block))
                        try bytes.withUnsafeMutableBytes { buffer in
                            try checked(CMBlockBufferCopyDataBytes(block, atOffset: 0,
                                dataLength: buffer.count, destination: buffer.baseAddress!), "read AU")
                        }
                        guard bytes == data,
                              CMTimeCompare(restored.presentationTimeStamp, CMTime(value: count, timescale: fps)) == 0 else {
                            throw ProbeFailure.message("passthrough bytes or PTS changed")
                        }
                        count += 1
                    }
                    guard reader.status == .completed, count == Int64(fps * 3) + 1 else {
                        throw reader.error ?? ProbeFailure.message("incomplete read")
                    }
                    print("PASS \(stem): \(count) identical AUs with exact PTS")
                }
            }
        }
    }
}
