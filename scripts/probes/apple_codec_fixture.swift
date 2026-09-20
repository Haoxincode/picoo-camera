// REQ-PICOO-NEXT-003/025: diagnostic hardware AVC/HEVC fixtures with explicit source meaning.
import Foundation
import CoreMedia
import CoreVideo
import VideoToolbox

struct FixtureError: Error { let message: String }
func check(_ status: OSStatus, _ operation: String) throws {
    guard status == noErr else { throw FixtureError(message: "\(operation): \(status)") }
}
final class Encoded: @unchecked Sendable {
    let lock = NSLock()
    var sample: CMSampleBuffer?
    var status: OSStatus = 0
    func receive(_ status: OSStatus, _ sample: CMSampleBuffer?) {
        lock.lock(); defer { lock.unlock() }
        self.status = status; self.sample = sample
    }
}
let callback: VTCompressionOutputCallback = { context, _, status, _, sample in
    guard let context else { return }
    Unmanaged<Encoded>.fromOpaque(context).takeUnretainedValue().receive(status, sample)
}

@MainActor
func makeFixture(codec: CMVideoCodecType, width: Int, height: Int, directory: URL) throws {
    let results = Encoded()
    var encoder: VTCompressionSession?
    try check(VTCompressionSessionCreate(allocator: nil, width: Int32(width), height: Int32(height),
        codecType: codec,
        encoderSpecification: [kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder: true] as CFDictionary,
        imageBufferAttributes: nil, compressedDataAllocator: nil, outputCallback: callback,
        refcon: Unmanaged.passUnretained(results).toOpaque(), compressionSessionOut: &encoder), "create hardware encoder")
    guard let encoder else { throw FixtureError(message: "no encoder") }
    defer { VTCompressionSessionInvalidate(encoder) }
    let aspect = [kCVImageBufferPixelAspectRatioHorizontalSpacingKey: 1, kCVImageBufferPixelAspectRatioVerticalSpacingKey: 1] as CFDictionary
    let properties: [(CFString, CFTypeRef)] = [
        (kVTCompressionPropertyKey_RealTime, kCFBooleanTrue!),
        (kVTCompressionPropertyKey_AllowFrameReordering, kCFBooleanFalse!),
        (kVTCompressionPropertyKey_ProfileLevel, codec == kCMVideoCodecType_HEVC ? kVTProfileLevel_HEVC_Main_AutoLevel : kVTProfileLevel_H264_High_AutoLevel),
        (kVTCompressionPropertyKey_ExpectedFrameRate, 30 as CFNumber),
        (kVTCompressionPropertyKey_ColorPrimaries, kCVImageBufferColorPrimaries_ITU_R_709_2),
        (kVTCompressionPropertyKey_TransferFunction, kCVImageBufferTransferFunction_ITU_R_709_2),
        (kVTCompressionPropertyKey_YCbCrMatrix, kCVImageBufferYCbCrMatrix_ITU_R_709_2),
        (kVTCompressionPropertyKey_PixelAspectRatio, aspect),
    ]
    for (key, value) in properties { try check(VTSessionSetProperty(encoder, key: key, value: value), "set \(key)") }
    try check(VTCompressionSessionPrepareToEncodeFrames(encoder), "prepare")
    var hardware: Unmanaged<CFTypeRef>?
    try check(VTSessionCopyProperty(encoder, key: kVTCompressionPropertyKey_UsingHardwareAcceleratedVideoEncoder, allocator: nil, valueOut: &hardware), "hardware evidence")
    guard (hardware?.takeRetainedValue() as? NSNumber)?.boolValue == true else { throw FixtureError(message: "software encoder") }
    var image: CVPixelBuffer?
    try check(CVPixelBufferCreate(nil, width, height, kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
        [kCVPixelBufferIOSurfacePropertiesKey: [:], kCVPixelBufferMetalCompatibilityKey: true] as CFDictionary, &image), "create diagnostic image")
    guard let image else { throw FixtureError(message: "no image") }
    for (key, value) in [(kCVImageBufferColorPrimariesKey, kCVImageBufferColorPrimaries_ITU_R_709_2),
                         (kCVImageBufferTransferFunctionKey, kCVImageBufferTransferFunction_ITU_R_709_2),
                         (kCVImageBufferYCbCrMatrixKey, kCVImageBufferYCbCrMatrix_ITU_R_709_2)] {
        CVBufferSetAttachment(image, key, value, .shouldPropagate)
    }
    CVBufferSetAttachment(image, kCVImageBufferPixelAspectRatioKey, aspect, .shouldPropagate)
    try check(CVPixelBufferLockBaseAddress(image, []), "diagnostic upload lock")
    for plane in 0..<2 {
        guard let base = CVPixelBufferGetBaseAddressOfPlane(image, plane) else { throw FixtureError(message: "no plane") }
        let stride = CVPixelBufferGetBytesPerRowOfPlane(image, plane)
        let rows = CVPixelBufferGetHeightOfPlane(image, plane)
        memset(base, plane == 0 ? 63 : 128, stride * rows)
        if plane == 1 {
            let bytes = base.assumingMemoryBound(to: UInt8.self)
            for y in 0..<rows { for x in Swift.stride(from: 0, to: width, by: 2) { bytes[y * stride + x] = 102; bytes[y * stride + x + 1] = 240 } }
        }
    }
    try check(CVPixelBufferUnlockBaseAddress(image, []), "diagnostic upload unlock")
    try check(VTCompressionSessionEncodeFrame(encoder, imageBuffer: image, presentationTimeStamp: .zero,
        duration: CMTime(value: 1, timescale: 30), frameProperties: nil, sourceFrameRefcon: nil, infoFlagsOut: nil), "encode")
    try check(VTCompressionSessionCompleteFrames(encoder, untilPresentationTimeStamp: .invalid), "complete")
    results.lock.lock(); defer { results.lock.unlock() }
    try check(results.status, "callback")
    guard let sample = results.sample, let format = CMSampleBufferGetFormatDescription(sample),
          let block = CMSampleBufferGetDataBuffer(sample) else { throw FixtureError(message: "no encoded sample") }
    var annex = Data()
    let codecName = codec == kCMVideoCodecType_HEVC ? "hevc" : "avc"
    let rawExtensions = CMFormatDescriptionGetExtensions(format)! as NSDictionary
    guard let atoms = rawExtensions[kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms] as? NSDictionary,
          let record = atoms[codec == kCMVideoCodecType_HEVC ? "hvcC" : "avcC"] as? Data else {
        throw FixtureError(message: "no native configuration record")
    }
    var parameterCount = 1
    var index = 0
    while index < parameterCount {
        var pointer: UnsafePointer<UInt8>?
        var count = 0
        let status = codec == kCMVideoCodecType_HEVC
            ? CMVideoFormatDescriptionGetHEVCParameterSetAtIndex(format, parameterSetIndex: index,
                parameterSetPointerOut: &pointer, parameterSetSizeOut: &count, parameterSetCountOut: &parameterCount,
                nalUnitHeaderLengthOut: nil)
            : CMVideoFormatDescriptionGetH264ParameterSetAtIndex(format, parameterSetIndex: index,
                parameterSetPointerOut: &pointer, parameterSetSizeOut: &count, parameterSetCountOut: &parameterCount,
                nalUnitHeaderLengthOut: nil)
        try check(status, "parameter set")
        guard let pointer, parameterCount > 0 && parameterCount <= 64 else { throw FixtureError(message: "invalid parameter sets") }
        annex.append(contentsOf: [0, 0, 0, 1]); annex.append(pointer, count: count)
        index += 1
    }
    let encodedLength = CMBlockBufferGetDataLength(block)
    var encoded = Data(count: encodedLength)
    try encoded.withUnsafeMutableBytes { try check(CMBlockBufferCopyDataBytes(block, atOffset: 0, dataLength: encodedLength, destination: $0.baseAddress!), "sample bytes") }
    var offset = 0
    while offset < encoded.count {
        guard offset + 4 <= encoded.count else { throw FixtureError(message: "truncated length") }
        let count = encoded[offset..<offset+4].reduce(0) { $0 * 256 + Int($1) }; offset += 4
        guard count > 0 && count <= encoded.count - offset else { throw FixtureError(message: "invalid NAL length") }
        annex.append(contentsOf: [0, 0, 0, 1]); annex.append(encoded[offset..<offset+count]); offset += count
    }
    let prefix = "\(codecName)-\(width)x\(height)-bt709"
    try record.write(to: directory.appendingPathComponent("\(prefix)-config.bin"))
    try encoded.write(to: directory.appendingPathComponent("\(prefix)-idr.bin"))
    let name = "\(prefix)-idr.\(codec == kCMVideoCodecType_HEVC ? "h265" : "h264")"
    try annex.write(to: directory.appendingPathComponent(name))
    print("\(name): \(annex.count) bytes, hardware \(codecName), BT.709 limited, square pixels")
}

guard CommandLine.arguments.count == 2 else { fatalError("Pass the fixture output directory") }
let directory = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
for codec in [kCMVideoCodecType_H264, kCMVideoCodecType_HEVC] {
    for (width, height) in [(64, 64), (1280, 720), (1920, 1080)] {
        try makeFixture(codec: codec, width: width, height: height, directory: directory)
    }
}
