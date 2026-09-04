import CoreMedia
import Foundation
import VideoToolbox

nonisolated struct EncodedFrameConfiguration: Sendable {
    let width: UInt32
    let height: UInt32
    let framesPerSecond: UInt32
    let bitrateBps: UInt32
    let streamEpoch: UInt32
    let encoderGeneration: UInt64
    let rotation: UInt32
}

nonisolated final class CompressionCallbackContext: @unchecked Sendable {
    private let configurationLock = NSLock()
    private var configuration: EncodedFrameConfiguration
    private let eventHandler: @Sendable (VideoEncoderEvent) -> Void

    init(
        configuration: EncodedFrameConfiguration,
        eventHandler: @escaping @Sendable (VideoEncoderEvent) -> Void
    ) {
        self.configuration = configuration
        self.eventHandler = eventHandler
    }

    func updateBitrate(_ bitrateBps: UInt32) {
        configurationLock.withLock {
            configuration = EncodedFrameConfiguration(
                width: configuration.width,
                height: configuration.height,
                framesPerSecond: configuration.framesPerSecond,
                bitrateBps: bitrateBps,
                streamEpoch: configuration.streamEpoch,
                encoderGeneration: configuration.encoderGeneration,
                rotation: configuration.rotation
            )
        }
    }

    func updateRotation(_ rotation: UInt32) {
        configurationLock.withLock {
            configuration = EncodedFrameConfiguration(
                width: configuration.width,
                height: configuration.height,
                framesPerSecond: configuration.framesPerSecond,
                bitrateBps: configuration.bitrateBps,
                streamEpoch: configuration.streamEpoch,
                encoderGeneration: configuration.encoderGeneration,
                rotation: rotation % 360
            )
        }
    }

    func receive(
        status: OSStatus,
        infoFlags: VTEncodeInfoFlags,
        sampleBuffer: CMSampleBuffer?
    ) {
        guard status == noErr else {
            let configuration = configurationLock.withLock { self.configuration }
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
            return
        }

        do {
            let configuration = configurationLock.withLock { self.configuration }
            let isKeyframe = Self.isKeyframe(sampleBuffer)
            let parameterSets = isKeyframe
                ? try Self.parameterSets(from: sampleBuffer.formatDescription)
                : nil
            let encodedData = try Self.encodedData(from: sampleBuffer)
            let data = parameterSets.map {
                Self.prependingParameterSets($0, to: encodedData)
            } ?? encodedData
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
                parameterSets: parameterSets
            )))
        } catch {
            let configuration = configurationLock.withLock { self.configuration }
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

    private static func parameterSets(
        from formatDescription: CMFormatDescription?
    ) throws -> H264ParameterSets? {
        guard let formatDescription else { return nil }
        var sequencePointer: UnsafePointer<UInt8>?
        var sequenceSize = 0
        var parameterSetCount = 0
        var nalUnitHeaderLength: Int32 = 0
        let sequenceStatus = CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
            formatDescription,
            parameterSetIndex: 0,
            parameterSetPointerOut: &sequencePointer,
            parameterSetSizeOut: &sequenceSize,
            parameterSetCountOut: &parameterSetCount,
            nalUnitHeaderLengthOut: &nalUnitHeaderLength
        )
        guard sequenceStatus == noErr,
              parameterSetCount >= 2,
              let sequencePointer,
              nalUnitHeaderLength == 4
        else {
            if sequenceStatus == noErr, nalUnitHeaderLength != 4 {
                throw VideoEncoderOutputError.unsupportedNALHeaderLength(
                    nalUnitHeaderLength
                )
            }
            return nil
        }

        var picturePointer: UnsafePointer<UInt8>?
        var pictureSize = 0
        let pictureStatus = CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
            formatDescription,
            parameterSetIndex: 1,
            parameterSetPointerOut: &picturePointer,
            parameterSetSizeOut: &pictureSize,
            parameterSetCountOut: nil,
            nalUnitHeaderLengthOut: nil
        )
        guard pictureStatus == noErr, let picturePointer else { return nil }
        return H264ParameterSets(
            sequence: Data(bytes: sequencePointer, count: sequenceSize),
            picture: Data(bytes: picturePointer, count: pictureSize)
        )
    }

    private static func encodedData(from sampleBuffer: CMSampleBuffer) throws -> Data {
        guard let blockBuffer = CMSampleBufferGetDataBuffer(sampleBuffer) else {
            throw VideoEncoderOutputError.missingBlockBuffer
        }
        let length = CMBlockBufferGetDataLength(blockBuffer)
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

    private static func prependingParameterSets(
        _ parameterSets: H264ParameterSets,
        to accessUnit: Data
    ) -> Data {
        var data = Data()
        data.reserveCapacity(
            8 + parameterSets.sequence.count + parameterSets.picture.count
                + accessUnit.count
        )
        appendAVCCNAL(parameterSets.sequence, to: &data)
        appendAVCCNAL(parameterSets.picture, to: &data)
        data.append(accessUnit)
        return data
    }

    private static func appendAVCCNAL(_ nal: Data, to data: inout Data) {
        var length = UInt32(nal.count).bigEndian
        withUnsafeBytes(of: &length) { data.append(contentsOf: $0) }
        data.append(nal)
    }

    private static func presentationTimeUs(_ time: CMTime) -> UInt64 {
        guard time.isValid, !time.isIndefinite else { return 0 }
        let converted = CMTimeConvertScale(time, timescale: 1_000_000, method: .default)
        return UInt64(max(0, converted.value))
    }
}

nonisolated private enum VideoEncoderOutputError: LocalizedError {
    case missingBlockBuffer
    case copyFailed(OSStatus)
    case unsupportedNALHeaderLength(Int32)

    var errorDescription: String? {
        switch self {
        case .missingBlockBuffer:
            "VideoToolbox 没有返回 H.264 数据"
        case let .copyFailed(status):
            "复制 H.264 Access Unit 失败（\(status)）"
        case let .unsupportedNALHeaderLength(length):
            "VideoToolbox 返回了不支持的 \(length) 字节 AVCC NAL 长度"
        }
    }
}
