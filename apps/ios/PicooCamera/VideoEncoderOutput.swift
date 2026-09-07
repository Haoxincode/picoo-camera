import CoreMedia
import Foundation
import VideoToolbox

nonisolated struct EncodedFrameConfiguration: Equatable, Sendable {
    let codec: NativeVideoCodec
    let width: UInt32
    let height: UInt32
    let framesPerSecond: UInt32
    let bitrateBps: UInt32
    let streamEpoch: UInt32
    let encoderGeneration: UInt64
    let rotation: UInt32
}

nonisolated final class CompressionCallbackContext: @unchecked Sendable {
    private let configuration: EncodedFrameConfiguration
    private let submittedFrames = SubmittedFrameConfigurations()
    private let eventHandler: @Sendable (VideoEncoderEvent) -> Void

    init(
        configuration: EncodedFrameConfiguration,
        eventHandler: @escaping @Sendable (VideoEncoderEvent) -> Void
    ) {
        self.configuration = configuration
        self.eventHandler = eventHandler
    }

    func reserveFrame(bitrateBps: UInt32, rotation: UInt32) -> UInt? {
        submittedFrames.reserve(EncodedFrameConfiguration(
            codec: configuration.codec,
            width: configuration.width,
            height: configuration.height,
            framesPerSecond: configuration.framesPerSecond,
            bitrateBps: bitrateBps,
            streamEpoch: configuration.streamEpoch,
            encoderGeneration: configuration.encoderGeneration,
            rotation: rotation
        ))
    }

    func cancelFrame(_ identifier: UInt) {
        _ = submittedFrames.take(identifier)
    }

    func receive(
        sourceFrameRefCon: UnsafeMutableRawPointer?,
        status: OSStatus,
        infoFlags: VTEncodeInfoFlags,
        sampleBuffer: CMSampleBuffer?
    ) {
        guard let sourceFrameRefCon,
              let configuration = submittedFrames.take(UInt(bitPattern: sourceFrameRefCon))
        else {
            eventHandler(.failure(
                streamEpoch: self.configuration.streamEpoch,
                encoderGeneration: self.configuration.encoderGeneration,
                message: "编码完成缺少有效的原始帧身份"
            ))
            return
        }
        guard status == noErr else {
            eventHandler(.failure(
                streamEpoch: configuration.streamEpoch,
                encoderGeneration: configuration.encoderGeneration,
                message: "VideoToolbox 输出失败（\(status)）"
            ))
            return
        }
        guard !infoFlags.contains(.frameDropped),
              let sampleBuffer,
              CMSampleBufferDataIsReady(sampleBuffer)
        else {
            eventHandler(.queueOverflow(
                streamEpoch: configuration.streamEpoch,
                encoderGeneration: configuration.encoderGeneration
            ))
            return
        }

        do {
            let isKeyframe = Self.isKeyframe(sampleBuffer)
            let codecConfiguration = try Self.codecConfiguration(from: sampleBuffer.formatDescription)
            guard codecConfiguration.codec == configuration.codec.rawValue else {
                throw VideoEncoderOutputError.configurationMismatch
            }
            let data = try Self.encodedData(from: sampleBuffer)
            let presentationTimeUs = Self.presentationTimeUs(
                sampleBuffer.presentationTimeStamp
            )
            eventHandler(.accessUnit(EncodedAccessUnit(
                data: data,
                isKeyframe: isKeyframe,
                presentationTimeUs: presentationTimeUs,
                encodedAtUs: Self.presentationTimeUs(
                    CMClockGetTime(CMClockGetHostTimeClock())
                ),
                width: configuration.width,
                height: configuration.height,
                framesPerSecond: configuration.framesPerSecond,
                bitrateBps: configuration.bitrateBps,
                streamEpoch: configuration.streamEpoch,
                encoderGeneration: configuration.encoderGeneration,
                rotation: configuration.rotation,
                codecConfiguration: codecConfiguration
            )))
        } catch {
            eventHandler(.failure(
                streamEpoch: configuration.streamEpoch,
                encoderGeneration: configuration.encoderGeneration,
                message: error.localizedDescription
            ))
        }
    }

    private static func isKeyframe(_ sampleBuffer: CMSampleBuffer) -> Bool {
        guard let attachments = CMSampleBufferGetSampleAttachmentsArray(
            sampleBuffer,
            createIfNecessary: false
        ) as? [[CFString: Any]],
            let first = attachments.first
        else {
            return true
        }
        return (first[kCMSampleAttachmentKey_NotSync] as? Bool) != true
    }

    static func codecConfiguration(from format: CMFormatDescription?) throws -> EncodedCodecConfiguration {
        guard let format else { throw VideoEncoderOutputError.missingConfiguration }
        let codec: UInt32
        let atom: String
        switch CMFormatDescriptionGetMediaSubType(format) {
        case kCMVideoCodecType_H264: codec = 1; atom = "avcC"
        case kCMVideoCodecType_HEVC: codec = 2; atom = "hvcC"
        default: throw VideoEncoderOutputError.missingConfiguration
        }
        guard let extensions = CMFormatDescriptionGetExtensions(format) as NSDictionary?,
              let atoms = extensions[kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms] as? NSDictionary,
              let record = atoms[atom] as? Data,
              !record.isEmpty, record.count <= 64 * 1024
        else { throw VideoEncoderOutputError.missingConfiguration }
        return EncodedCodecConfiguration(codec: codec, record: record)
    }

    private static func encodedData(from sampleBuffer: CMSampleBuffer) throws -> Data {
        guard let blockBuffer = CMSampleBufferGetDataBuffer(sampleBuffer) else {
            throw VideoEncoderOutputError.missingBlockBuffer
        }
        let length = CMBlockBufferGetDataLength(blockBuffer)
        guard length > 0, length <= 2 * 1024 * 1024 else { throw VideoEncoderOutputError.excessiveAccessUnit }
        var data = Data(count: length)
        let status = data.withUnsafeMutableBytes { bytes in
            guard let destination = bytes.baseAddress else { return kCMBlockBufferBadPointerParameterErr }
            return CMBlockBufferCopyDataBytes(
                blockBuffer,
                atOffset: 0,
                dataLength: length,
                destination: destination
            )
        }
        guard status == kCMBlockBufferNoErr else {
            throw VideoEncoderOutputError.copyFailed(status)
        }
        return data
    }

    private static func presentationTimeUs(_ time: CMTime) -> UInt64 {
        guard time.isValid, !time.isIndefinite else { return 0 }
        let converted = CMTimeConvertScale(time, timescale: 1_000_000, method: .default)
        return UInt64(max(0, converted.value))
    }
}

nonisolated private enum VideoEncoderOutputError: LocalizedError {
    case configurationMismatch
    case missingBlockBuffer
    case copyFailed(OSStatus)
    case missingConfiguration
    case excessiveAccessUnit

    var errorDescription: String? {
        switch self {
        case .configurationMismatch:
            "VideoToolbox 输出 codec 与请求不一致"
        case .missingBlockBuffer:
            "VideoToolbox 没有返回压缩数据"
        case let .copyFailed(status):
            "复制压缩 Access Unit 失败（\(status)）"
        case .missingConfiguration:
            "VideoToolbox 没有返回有效配置记录"
        case .excessiveAccessUnit:
            "VideoToolbox 输出数据超出容量限制"
        }
    }
}

/// Opaque native callback IDs own immutable input facts, never current owner state.
/// A missing/duplicate completion cannot borrow another submission's identity.
nonisolated final class SubmittedFrameConfigurations: @unchecked Sendable {
    private let lock = NSLock()
    private var nextIdentifier: UInt = 1
    private var pending: [UInt: EncodedFrameConfiguration] = [:]

    func reserve(_ configuration: EncodedFrameConfiguration) -> UInt? {
        lock.withLock {
            guard pending.count < 16, nextIdentifier < UInt.max else { return nil }
            let identifier = nextIdentifier
            nextIdentifier += 1
            pending[identifier] = configuration
            return identifier
        }
    }

    func take(_ identifier: UInt) -> EncodedFrameConfiguration? {
        lock.withLock { pending.removeValue(forKey: identifier) }
    }
}
