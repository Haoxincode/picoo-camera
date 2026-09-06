// REQ-PICOO-NEXT-003/009/016: executable platform contract probe, not product code.
import CoreMedia
import CoreVideo
import Foundation
import Metal
import VideoToolbox

struct ProbeError: Error, CustomStringConvertible {
    let description: String
    init(_ description: String) { self.description = description }
}
func require(_ condition: @autoclosure () throws -> Bool, _ message: String) throws {
    if try !condition() { throw ProbeError(message) }
}
func check(_ status: OSStatus, _ operation: String) throws {
    try require(status == noErr, "\(operation): OSStatus \(status)")
}
final class SampleResults: @unchecked Sendable {
    private let lock = NSLock()
    private var samples: [CMSampleBuffer] = []
    private var images: [CVPixelBuffer] = []
    private var failure: OSStatus = 0
    func encoded(_ status: OSStatus, _ sample: CMSampleBuffer?) {
        lock.lock(); defer { lock.unlock() }
        if status != 0 { failure = status }
        if let sample { samples.append(sample) }
    }
    func decoded(_ status: OSStatus, _ image: CVPixelBuffer?) {
        lock.lock(); defer { lock.unlock() }
        if status != 0 { failure = status }
        if let image { images.append(image) }
    }
    func snapshot() throws -> ([CMSampleBuffer], [CVPixelBuffer]) {
        lock.lock(); defer { lock.unlock() }
        try check(failure, "codec callback")
        return (samples, images)
    }
}
let encodeCallback: VTCompressionOutputCallback = { context, _, status, _, sample in
    guard let context else { return }
    Unmanaged<SampleResults>.fromOpaque(context).takeUnretainedValue().encoded(status, sample)
}
let decodeCallback: VTDecompressionOutputCallback = { context, _, status, _, image, _, _ in
    guard let context else { return }
    Unmanaged<SampleResults>.fromOpaque(context).takeUnretainedValue().decoded(status, image)
}
func attributes(_ width: Int, _ height: Int) -> CFDictionary {
    [kCVPixelBufferWidthKey: width, kCVPixelBufferHeightKey: height,
     kCVPixelBufferPixelFormatTypeKey: kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
     kCVPixelBufferIOSurfacePropertiesKey: [:], kCVPixelBufferMetalCompatibilityKey: true] as CFDictionary
}
func makeFixture(_ width: Int, _ height: Int) throws -> CVPixelBuffer {
    var image: CVPixelBuffer?
    try check(CVPixelBufferCreate(nil, width, height,
        kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange, attributes(width, height), &image), "create fixture")
    guard let image else { throw ProbeError("no fixture") }
    // Explicit diagnostic initialization, not a production CPU video path.
    try check(CVPixelBufferLockBaseAddress(image, []), "lock fixture")
    defer { CVPixelBufferUnlockBaseAddress(image, []) }
    for plane in 0..<2 {
        guard let base = CVPixelBufferGetBaseAddressOfPlane(image, plane) else { throw ProbeError("fixture plane") }
        memset(base, plane == 0 ? 96 : 128,
               CVPixelBufferGetBytesPerRowOfPlane(image, plane) * CVPixelBufferGetHeightOfPlane(image, plane))
    }
    return image
}
func hardware(_ session: VTSession, _ key: CFString) throws -> Bool {
    var value: Unmanaged<CFTypeRef>?
    try check(VTSessionCopyProperty(session, key: key, allocator: nil, valueOut: &value), "hardware evidence")
    return (value?.takeRetainedValue() as? NSNumber)?.boolValue == true
}
// Immutable submission ownership only; no encoder/context API crosses threads.
final class SubmissionLease: @unchecked Sendable {
    let image: CVPixelBuffer
    let views: [CVMetalTexture]
    let targets: [MTLTexture]
    init(_ image: CVPixelBuffer, _ views: [CVMetalTexture], _ targets: [MTLTexture]) {
        self.image = image; self.views = views; self.targets = targets
    }
}
func verifyNative(_ image: CVPixelBuffer, device: MTLDevice, width: Int, height: Int) throws {
    try require(CVPixelBufferGetWidth(image) == width && CVPixelBufferGetHeight(image) == height, "decoded dimensions")
    try require(CVPixelBufferGetPixelFormatType(image) == kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange, "decoded format")
    try require(CVPixelBufferGetIOSurface(image) != nil, "decoded image lacks IOSurface")
    var cache: CVMetalTextureCache?
    try check(CVMetalTextureCacheCreate(nil, nil, device, nil, &cache), "texture cache")
    guard let cache, let queue = device.makeCommandQueue(), let command = queue.makeCommandBuffer(),
          let blit = command.makeBlitCommandEncoder() else { throw ProbeError("Metal allocation") }
    var views: [CVMetalTexture] = []
    var destinations: [MTLTexture] = []
    for plane in 0..<2 {
        let w = CVPixelBufferGetWidthOfPlane(image, plane)
        let h = CVPixelBufferGetHeightOfPlane(image, plane)
        let format: MTLPixelFormat = plane == 0 ? .r8Unorm : .rg8Unorm
        var view: CVMetalTexture?
        try check(CVMetalTextureCacheCreateTextureFromImage(nil, cache, image, nil, format, w, h, plane, &view), "native texture plane \(plane)")
        guard let view, let source = CVMetalTextureGetTexture(view) else { throw ProbeError("no native texture") }
        let desc = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: format, width: w, height: h, mipmapped: false)
        desc.storageMode = .private
        guard let target = device.makeTexture(descriptor: desc) else { throw ProbeError("no target texture") }
        blit.copy(from: source, sourceSlice: 0, sourceLevel: 0, sourceOrigin: MTLOrigin(x: 0, y: 0, z: 0),
                  sourceSize: MTLSize(width: w, height: h, depth: 1), to: target, destinationSlice: 0,
                  destinationLevel: 0, destinationOrigin: MTLOrigin(x: 0, y: 0, z: 0))
        views.append(view); destinations.append(target)
    }
    blit.endEncoding()
    // Probe thread only. Retain PixelBuffer + CVMetalTexture + target through actual completion.
    let completed = DispatchSemaphore(value: 0)
    let lease = SubmissionLease(image, views, destinations)
    command.addCompletedHandler { _ in
        withExtendedLifetime(lease) { _ = completed.signal() }
    }
    command.commit()
    try withExtendedLifetime((image, views, destinations)) {
        try require(completed.wait(timeout: .now() + 10) == .success, "GPU completion deadline")
        try require(command.status == .completed, "GPU failed: \(String(describing: command.error))")
    }
}
func verifyCpuContainer(_ image: CVPixelBuffer, width: Int, height: Int) throws {
    var target: CVPixelBuffer?
    try check(CVPixelBufferCreate(nil, width, height, kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
                                 attributes(width, height), &target), "CPU target")
    guard let target else { throw ProbeError("no CPU target") }
    try check(CVPixelBufferLockBaseAddress(image, .readOnly), "CPU source read lock")
    defer { CVPixelBufferUnlockBaseAddress(image, .readOnly) }
    try check(CVPixelBufferLockBaseAddress(target, []), "CPU target write lock")
    defer { CVPixelBufferUnlockBaseAddress(target, []) }
    for plane in 0..<2 {
        guard let src = CVPixelBufferGetBaseAddressOfPlane(image, plane),
              let dst = CVPixelBufferGetBaseAddressOfPlane(target, plane) else { throw ProbeError("CPU plane") }
        let srcStride = CVPixelBufferGetBytesPerRowOfPlane(image, plane)
        let dstStride = CVPixelBufferGetBytesPerRowOfPlane(target, plane)
        try require(srcStride >= width && dstStride >= width, "plane stride")
        for row in 0..<(plane == 0 ? height : height / 2) {
            memcpy(dst.advanced(by: row * dstStride), src.advanced(by: row * srcStride), width)
            try require(memcmp(dst.advanced(by: row * dstStride), src.advanced(by: row * srcStride), width) == 0, "CPU plane mismatch")
        }
    }
    var format: CMVideoFormatDescription?
    try check(CMVideoFormatDescriptionCreateForImageBuffer(allocator: nil, imageBuffer: target, formatDescriptionOut: &format), "CPU container description")
    var sample: CMSampleBuffer?
    var timing = CMSampleTimingInfo(duration: CMTime(value: 1, timescale: 60), presentationTimeStamp: .zero, decodeTimeStamp: .invalid)
    try check(CMSampleBufferCreateReadyWithImageBuffer(allocator: nil, imageBuffer: target, formatDescription: format!, sampleTiming: &timing, sampleBufferOut: &sample), "CPU container sample")
    try require(sample != nil && CVPixelBufferGetIOSurface(target) != nil, "CPU native container")
}
@MainActor
func runProbe(codec: CMVideoCodecType, name: String, width: Int, height: Int, fps: Int, device: MTLDevice) throws -> [String: Any] {
    let encoded = SampleResults()
    var encoder: VTCompressionSession?
    try check(VTCompressionSessionCreate(allocator: nil, width: Int32(width), height: Int32(height), codecType: codec,
        encoderSpecification: [kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder: true] as CFDictionary,
        imageBufferAttributes: attributes(width, height), compressedDataAllocator: nil, outputCallback: encodeCallback,
        refcon: Unmanaged.passUnretained(encoded).toOpaque(), compressionSessionOut: &encoder), "create \(name) encoder")
    guard let encoder else { throw ProbeError("no encoder") }
    defer { VTCompressionSessionInvalidate(encoder) }
    for (key, value) in [(kVTCompressionPropertyKey_RealTime, true as CFTypeRef),
                         (kVTCompressionPropertyKey_AllowFrameReordering, false as CFTypeRef),
                         (kVTCompressionPropertyKey_ExpectedFrameRate, fps as CFTypeRef),
                         (kVTCompressionPropertyKey_ProfileLevel, (codec == kCMVideoCodecType_H264 ? kVTProfileLevel_H264_High_AutoLevel : kVTProfileLevel_HEVC_Main_AutoLevel) as CFTypeRef)] {
        try check(VTSessionSetProperty(encoder, key: key, value: value), "encoder property \(key)")
    }
    try check(VTCompressionSessionPrepareToEncodeFrames(encoder), "prepare encoder")
    let fixture = try makeFixture(width, height)
    for ix in 0..<3 {
        try check(VTCompressionSessionEncodeFrame(encoder, imageBuffer: fixture, presentationTimeStamp: CMTime(value: Int64(ix), timescale: Int32(fps)),
            duration: CMTime(value: 1, timescale: Int32(fps)), frameProperties: nil, sourceFrameRefcon: nil, infoFlagsOut: nil), "encode")
    }
    try check(VTCompressionSessionCompleteFrames(encoder, untilPresentationTimeStamp: .invalid), "drain encoder")
    let samples = try encoded.snapshot().0
    try require(samples.count == 3, "encoder returned \(samples.count)/3 samples")
    try require(try hardware(encoder, kVTCompressionPropertyKey_UsingHardwareAcceleratedVideoEncoder), "software encoder")
    guard let format = CMSampleBufferGetFormatDescription(samples[0]) else { throw ProbeError("no codec description") }
    if CommandLine.arguments.count > 1 && width == 1280 && fps == 30 {
        let directory = URL(fileURLWithPath: CommandLine.arguments[1])
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        guard let rawExtensions = CMFormatDescriptionGetExtensions(format) else { throw ProbeError("missing format extensions") }
        let extensions = rawExtensions as NSDictionary
        guard let atoms = extensions[kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms] as? NSDictionary,
              let record = atoms[name == "avc" ? "avcC" : "hvcC"] as? Data,
              let block = CMSampleBufferGetDataBuffer(samples[0]) else { throw ProbeError("missing configuration atom/AU") }
        var au = Data(count: CMBlockBufferGetDataLength(block))
        let length = au.count
        try au.withUnsafeMutableBytes { raw in
            try check(CMBlockBufferCopyDataBytes(block, atOffset: 0, dataLength: length, destination: raw.baseAddress!), "copy compressed fixture")
        }
        try record.write(to: directory.appendingPathComponent("\(name)-720p-config.bin"))
        try au.write(to: directory.appendingPathComponent("\(name)-720p-idr.bin"))
    }
    let decoded = SampleResults()
    var decoder: VTDecompressionSession?
    var callback = VTDecompressionOutputCallbackRecord(decompressionOutputCallback: decodeCallback, decompressionOutputRefCon: Unmanaged.passUnretained(decoded).toOpaque())
    try check(VTDecompressionSessionCreate(allocator: nil, formatDescription: format,
        decoderSpecification: [kVTVideoDecoderSpecification_RequireHardwareAcceleratedVideoDecoder: true] as CFDictionary,
        imageBufferAttributes: attributes(width, height), outputCallback: &callback, decompressionSessionOut: &decoder), "create decoder")
    guard let decoder else { throw ProbeError("no decoder") }
    defer { VTDecompressionSessionInvalidate(decoder) }
    for sample in samples {
        try check(VTDecompressionSessionDecodeFrame(decoder, sampleBuffer: sample, flags: [], frameRefcon: nil, infoFlagsOut: nil), "decode")
    }
    try check(VTDecompressionSessionWaitForAsynchronousFrames(decoder), "drain decoder")
    let images = try decoded.snapshot().1
    try require(images.count == 3, "decoder returned \(images.count)/3 images")
    try require(try hardware(decoder, kVTDecompressionPropertyKey_UsingHardwareAcceleratedVideoDecoder), "software decoder")
    for image in images { try verifyNative(image, device: device, width: width, height: height) }
    try verifyCpuContainer(images[0], width: width, height: height)
    return ["codec": name, "width": width, "height": height, "requested_fps": fps,
            "encoded_samples": samples.count, "decoded_images": images.count,
            "hardware_encoder": true, "hardware_decoder": true, "iosurface_metal_planes": true,
            "gpu_blit_completed": true, "cpu_container_plane_copy": true,
            "cmio_delivery": "not_tested", "steady_state_fps": "not_measured"]
}

do {
    guard let device = MTLCreateSystemDefaultDevice() else { throw ProbeError("no Metal device") }
    var results: [[String: Any]] = []
    for (codec, name) in [(kCMVideoCodecType_H264, "avc"), (kCMVideoCodecType_HEVC, "hevc")] {
        for (width, height) in [(1280, 720), (1920, 1080)] {
            for fps in [30, 60] {
                results.append(try runProbe(codec: codec, name: name, width: width, height: height, fps: fps, device: device))
            }
        }
    }
    let report: [String: Any] = ["device": device.name, "os": ProcessInfo.processInfo.operatingSystemVersionString,
                               "scope": "synthetic hardware contract; not end-to-end or performance acceptance", "results": results]
    print(String(data: try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys]), encoding: .utf8)!)
} catch {
    fputs("Apple native media probe failed: \(error)\n", stderr)
    exit(1)
}
