import AVFoundation
import CoreMedia
import CoreVideo
import Foundation
import VideoToolbox

// REQ-PICOO-MEDIA-011: native 420v capture -> hardware H.264 Access Units.

nonisolated enum VideoResolution: Int, Sendable {
    case p480 = 480
    case p720 = 720
    case p1080 = 1080

    var width: Int32 {
        switch self {
        case .p480: 854
        case .p720: 1280
        case .p1080: 1920
        }
    }

    var height: Int32 { Int32(rawValue) }

    static func supported(forRequestedHeight height: UInt32) -> Self {
        if height >= 1080 { return .p1080 }
        if height >= 720 { return .p720 }
        return .p480
    }

    func clamped(toMaximumHeight maximumHeight: UInt32) -> Self {
        guard maximumHeight > 0,
              UInt32(rawValue) > maximumHeight
        else {
            return self
        }
        return Self.supported(forRequestedHeight: maximumHeight)
    }
}

nonisolated enum VideoBitrate {
    static func initial(for resolution: VideoResolution) -> UInt32 {
        switch resolution {
        case .p480: 1_800_000
        case .p720: 3_000_000
        case .p1080: 6_000_000
        }
    }

    static func clamp(_ bitrate: UInt32, for resolution: VideoResolution) -> UInt32 {
        let range: ClosedRange<UInt32> = switch resolution {
        case .p480: 900_000 ... 2_500_000
        case .p720: 1_500_000 ... 5_000_000
        case .p1080: 3_000_000 ... 10_000_000
        }
        return min(max(bitrate, range.lowerBound), range.upperBound)
    }
}

nonisolated enum StreamEpoch {
    static let initial: UInt32 = 1

    static func bump(_ current: UInt32) -> UInt32 {
        current == .max ? initial : current + 1
    }
}

nonisolated struct VideoEncoderConfiguration: Equatable, Sendable {
    let resolution: VideoResolution
    let framesPerSecond: UInt32
    let bitrateBps: UInt32
    let streamEpoch: UInt32
    let rotation: UInt32

    init(
        resolution: VideoResolution,
        framesPerSecond: UInt32 = 30,
        bitrateBps: UInt32? = nil,
        streamEpoch: UInt32 = StreamEpoch.initial,
        rotation: UInt32 = 0
    ) {
        self.resolution = resolution
        self.framesPerSecond = framesPerSecond
        self.bitrateBps = VideoBitrate.clamp(
            bitrateBps ?? VideoBitrate.initial(for: resolution),
            for: resolution
        )
        self.streamEpoch = streamEpoch
        self.rotation = rotation % 360
    }
}

nonisolated struct H264ParameterSets: Equatable, Sendable {
    let sequence: Data
    let picture: Data
}

nonisolated struct EncodedAccessUnit: Equatable, Sendable {
    let data: Data
    let isKeyframe: Bool
    let presentationTimeUs: UInt64
    let width: UInt32
    let height: UInt32
    let framesPerSecond: UInt32
    let bitrateBps: UInt32
    let streamEpoch: UInt32
    let rotation: UInt32
    let parameterSets: H264ParameterSets?
}

nonisolated enum VideoEncoderEvent: Sendable {
    case accessUnit(EncodedAccessUnit)
    case failure(String)
    case queueOverflow
}

/// Small bounded GOP-aware handoff between VideoToolbox and the Rust sender.
/// If the consumer falls behind, dependent frames are discarded until a new
/// IDR arrives. Only wake-up tokens use AsyncStream; H.264 Data stays bounded.
nonisolated final class VideoEncoderEventBuffer: @unchecked Sendable {
    let signals: AsyncStream<Void>

    private let capacity: Int
    private let lock = NSLock()
    private let signalContinuation: AsyncStream<Void>.Continuation
    private var events: [VideoEncoderEvent] = []
    private var waitingForKeyframe = false

    init(capacity: Int = 12) {
        precondition(capacity >= 2)
        self.capacity = capacity
        let signals = AsyncStream.makeStream(
            of: Void.self,
            bufferingPolicy: .bufferingNewest(1)
        )
        self.signals = signals.stream
        signalContinuation = signals.continuation
    }

    func enqueue(_ event: VideoEncoderEvent) {
        let didEnqueue = lock.withLock {
            switch event {
            case let .accessUnit(accessUnit):
                if waitingForKeyframe {
                    guard accessUnit.isKeyframe else { return false }
                    events.append(event)
                    waitingForKeyframe = false
                    return true
                }
                guard events.count < capacity else {
                    events.removeAll(keepingCapacity: true)
                    events.append(.queueOverflow)
                    waitingForKeyframe = !accessUnit.isKeyframe
                    if accessUnit.isKeyframe {
                        events.append(event)
                    }
                    return true
                }
                events.append(event)
                return true
            case .failure, .queueOverflow:
                if events.count >= capacity {
                    events.removeAll(keepingCapacity: true)
                }
                events.append(event)
                return true
            }
        }
        if didEnqueue {
            signalContinuation.yield(())
        }
    }

    func drain() -> [VideoEncoderEvent] {
        lock.withLock {
            let drained = events
            events.removeAll(keepingCapacity: true)
            return drained
        }
    }

    func discardUntilKeyframe() {
        lock.withLock {
            events.removeAll(keepingCapacity: true)
            waitingForKeyframe = true
        }
    }
}

nonisolated enum VideoEncoderError: LocalizedError {
    case sessionCreation(OSStatus)
    case property(String, OSStatus)
    case prepare(OSStatus)
    case pixelBufferPoolUnavailable
    case pixelBufferCreation(CVReturn)
    case pixelTransferCreation(OSStatus)
    case pixelTransfer(OSStatus)

    var errorDescription: String? {
        switch self {
        case let .sessionCreation(status):
            "无法创建硬件 H.264 编码器（\(status)）"
        case let .property(key, status):
            "无法配置 H.264 编码参数 \(key)（\(status)）"
        case let .prepare(status):
            "H.264 编码器准备失败（\(status)）"
        case .pixelBufferPoolUnavailable:
            "H.264 编码器没有可用的缩放缓冲池"
        case let .pixelBufferCreation(status):
            "无法创建 H.264 缩放缓冲（\(status)）"
        case let .pixelTransferCreation(status):
            "无法创建视频缩放器（\(status)）"
        case let .pixelTransfer(status):
            "视频缩放失败（\(status)）"
        }
    }
}

/// All mutable encoder state is confined to `callbackQueue`. VideoToolbox may
/// invoke its output callback on another thread, so that callback only creates
/// immutable `Data` and emits a Sendable event.
nonisolated final class VideoEncoderPipeline: NSObject,
    AVCaptureVideoDataOutputSampleBufferDelegate,
    @unchecked Sendable
{
    let callbackQueue = DispatchQueue(
        label: "camera.picoo.ios.capture-and-encode",
        qos: .userInitiated
    )

    private let eventHandler: @Sendable (VideoEncoderEvent) -> Void
    private var configuration = VideoEncoderConfiguration(resolution: .p1080)
    private var compressionSession: VTCompressionSession?
    private var compressionContext: CompressionCallbackContext?
    private var pixelTransferSession: VTPixelTransferSession?
    private var isAcceptingFrames = false
    private var forceNextKeyframe = true

    init(eventHandler: @escaping @Sendable (VideoEncoderEvent) -> Void) {
        self.eventHandler = eventHandler
        super.init()
    }

    func start(configuration: VideoEncoderConfiguration) async {
        await perform {
            self.configuration = configuration
            self.forceNextKeyframe = true
            self.isAcceptingFrames = true
        }
    }

    func pause() async {
        await perform {
            self.isAcceptingFrames = false
            self.invalidateCompressionSession()
        }
    }

    func updateBitrate(_ bitrateBps: UInt32) async {
        await perform {
            let clamped = VideoBitrate.clamp(
                bitrateBps,
                for: self.configuration.resolution
            )
            self.configuration = VideoEncoderConfiguration(
                resolution: self.configuration.resolution,
                framesPerSecond: self.configuration.framesPerSecond,
                bitrateBps: clamped,
                streamEpoch: self.configuration.streamEpoch,
                rotation: self.configuration.rotation
            )
            guard let session = self.compressionSession else { return }
            do {
                try Self.setBitrate(clamped, on: session)
                self.compressionContext?.updateBitrate(clamped)
            } catch {
                self.eventHandler(.failure(error.localizedDescription))
            }
        }
    }

    func updateRotation(_ rotation: UInt32) async {
        await perform {
            self.configuration = VideoEncoderConfiguration(
                resolution: self.configuration.resolution,
                framesPerSecond: self.configuration.framesPerSecond,
                bitrateBps: self.configuration.bitrateBps,
                streamEpoch: self.configuration.streamEpoch,
                rotation: rotation
            )
            self.compressionContext?.updateRotation(rotation)
        }
    }

    func requestKeyframe() async {
        await perform { self.forceNextKeyframe = true }
    }

    func stop() async {
        await perform {
            self.isAcceptingFrames = false
            self.invalidateCompressionSession()
        }
    }

    private func perform(_ operation: @escaping @Sendable () -> Void) async {
        await withCheckedContinuation { continuation in
            callbackQueue.async {
                operation()
                continuation.resume()
            }
        }
    }

    private func invalidateCompressionSession() {
        guard let compressionSession else { return }
        VTCompressionSessionCompleteFrames(
            compressionSession,
            untilPresentationTimeStamp: .invalid
        )
        VTCompressionSessionInvalidate(compressionSession)
        self.compressionSession = nil
        compressionContext = nil
        if let pixelTransferSession {
            VTPixelTransferSessionInvalidate(pixelTransferSession)
            self.pixelTransferSession = nil
        }
    }

    private func compressionSession(for imageBuffer: CVImageBuffer) throws -> VTCompressionSession {
        if let compressionSession {
            return compressionSession
        }

        let (outputWidth, outputHeight) = outputDimensions(for: imageBuffer)

        let actualConfiguration = EncodedFrameConfiguration(
            width: UInt32(outputWidth),
            height: UInt32(outputHeight),
            framesPerSecond: configuration.framesPerSecond,
            bitrateBps: configuration.bitrateBps,
            streamEpoch: configuration.streamEpoch,
            rotation: configuration.rotation
        )
        let context = CompressionCallbackContext(
            configuration: actualConfiguration,
            eventHandler: eventHandler
        )
        var session: VTCompressionSession?
        let encoderSpecification: CFDictionary = [
            kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder: true,
        ] as CFDictionary
        let imageBufferAttributes: CFDictionary = [
            kCVPixelBufferPixelFormatTypeKey: kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
            kCVPixelBufferWidthKey: outputWidth,
            kCVPixelBufferHeightKey: outputHeight,
            kCVPixelBufferIOSurfacePropertiesKey: [:] as CFDictionary,
        ] as CFDictionary

        let status = VTCompressionSessionCreate(
            allocator: kCFAllocatorDefault,
            width: outputWidth,
            height: outputHeight,
            codecType: kCMVideoCodecType_H264,
            encoderSpecification: encoderSpecification,
            imageBufferAttributes: imageBufferAttributes,
            compressedDataAllocator: nil,
            outputCallback: Self.outputCallback,
            refcon: Unmanaged.passUnretained(context).toOpaque(),
            compressionSessionOut: &session
        )
        guard status == noErr, let session else {
            throw VideoEncoderError.sessionCreation(status)
        }

        do {
            try Self.configure(session, configuration: actualConfiguration)
        } catch {
            VTCompressionSessionInvalidate(session)
            throw error
        }

        compressionContext = context
        compressionSession = session
        return session
    }

    private func outputDimensions(for imageBuffer: CVImageBuffer) -> (Int32, Int32) {
        let width = CVPixelBufferGetWidth(imageBuffer)
        let height = CVPixelBufferGetHeight(imageBuffer)
        if width >= height {
            return (configuration.resolution.width, configuration.resolution.height)
        }
        return (configuration.resolution.height, configuration.resolution.width)
    }

    private func imageBufferForEncoding(
        _ source: CVImageBuffer,
        session: VTCompressionSession
    ) throws -> CVImageBuffer {
        let (targetWidth, targetHeight) = outputDimensions(for: source)
        guard CVPixelBufferGetWidth(source) != Int(targetWidth)
                || CVPixelBufferGetHeight(source) != Int(targetHeight)
        else {
            return source
        }
        guard let pool = VTCompressionSessionGetPixelBufferPool(session) else {
            throw VideoEncoderError.pixelBufferPoolUnavailable
        }
        var destination: CVPixelBuffer?
        let bufferStatus = CVPixelBufferPoolCreatePixelBuffer(
            kCFAllocatorDefault,
            pool,
            &destination
        )
        guard bufferStatus == kCVReturnSuccess, let destination else {
            throw VideoEncoderError.pixelBufferCreation(bufferStatus)
        }

        let transferSession: VTPixelTransferSession
        if let pixelTransferSession {
            transferSession = pixelTransferSession
        } else {
            var created: VTPixelTransferSession?
            let createStatus = VTPixelTransferSessionCreate(
                allocator: kCFAllocatorDefault,
                pixelTransferSessionOut: &created
            )
            guard createStatus == noErr, let created else {
                throw VideoEncoderError.pixelTransferCreation(createStatus)
            }
            try Self.set(
                kVTPixelTransferPropertyKey_ScalingMode,
                value: kVTScalingMode_Trim,
                on: created
            )
            try Self.set(
                kVTPixelTransferPropertyKey_RealTime,
                value: kCFBooleanTrue,
                on: created
            )
            pixelTransferSession = created
            transferSession = created
        }
        let transferStatus = VTPixelTransferSessionTransferImage(
            transferSession,
            from: source,
            to: destination
        )
        guard transferStatus == noErr else {
            throw VideoEncoderError.pixelTransfer(transferStatus)
        }
        return destination
    }

    private static func configure(
        _ session: VTCompressionSession,
        configuration: EncodedFrameConfiguration
    ) throws {
        try set(kVTCompressionPropertyKey_RealTime, value: kCFBooleanTrue, on: session)
        try set(
            kVTCompressionPropertyKey_AllowFrameReordering,
            value: kCFBooleanFalse,
            on: session
        )
        try set(
            kVTCompressionPropertyKey_ExpectedFrameRate,
            value: NSNumber(value: configuration.framesPerSecond),
            on: session
        )
        try set(
            kVTCompressionPropertyKey_MaxKeyFrameInterval,
            value: NSNumber(value: configuration.framesPerSecond * 2),
            on: session
        )
        try set(
            kVTCompressionPropertyKey_MaxKeyFrameIntervalDuration,
            value: NSNumber(value: 2),
            on: session
        )

        let mainProfileStatus = VTSessionSetProperty(
            session,
            key: kVTCompressionPropertyKey_ProfileLevel,
            value: kVTProfileLevel_H264_Main_4_0
        )
        if mainProfileStatus != noErr {
            try set(
                kVTCompressionPropertyKey_ProfileLevel,
                value: kVTProfileLevel_H264_Baseline_4_0,
                on: session
            )
        }
        try setBitrate(configuration.bitrateBps, on: session)

        let prepareStatus = VTCompressionSessionPrepareToEncodeFrames(session)
        guard prepareStatus == noErr else {
            throw VideoEncoderError.prepare(prepareStatus)
        }
    }

    private static func setBitrate(
        _ bitrateBps: UInt32,
        on session: VTCompressionSession
    ) throws {
        try set(
            kVTCompressionPropertyKey_AverageBitRate,
            value: NSNumber(value: bitrateBps),
            on: session
        )
        let bytesPerSecond = max(1, bitrateBps / 8)
        try set(
            kVTCompressionPropertyKey_DataRateLimits,
            value: [NSNumber(value: bytesPerSecond), NSNumber(value: 1)] as CFArray,
            on: session
        )
    }

    private static func set(
        _ key: CFString,
        value: CFTypeRef,
        on session: VTCompressionSession
    ) throws {
        let status = VTSessionSetProperty(session, key: key, value: value)
        guard status == noErr else {
            throw VideoEncoderError.property(key as String, status)
        }
    }

    private static func set(
        _ key: CFString,
        value: CFTypeRef,
        on session: VTPixelTransferSession
    ) throws {
        let status = VTSessionSetProperty(session, key: key, value: value)
        guard status == noErr else {
            throw VideoEncoderError.property(key as String, status)
        }
    }

    private static let outputCallback: VTCompressionOutputCallback = {
        outputCallbackRefCon,
        _,
        status,
        infoFlags,
        sampleBuffer in
        guard let outputCallbackRefCon else { return }
        let context = Unmanaged<CompressionCallbackContext>
            .fromOpaque(outputCallbackRefCon)
            .takeUnretainedValue()
        context.receive(status: status, infoFlags: infoFlags, sampleBuffer: sampleBuffer)
    }
}

extension VideoEncoderPipeline {
    nonisolated func captureOutput(
        _ output: AVCaptureOutput,
        didOutput sampleBuffer: CMSampleBuffer,
        from connection: AVCaptureConnection
    ) {
        guard isAcceptingFrames,
              let imageBuffer = CMSampleBufferGetImageBuffer(sampleBuffer)
        else {
            return
        }

        do {
            let session = try compressionSession(for: imageBuffer)
            let encodingBuffer = try imageBufferForEncoding(
                imageBuffer,
                session: session
            )
            var infoFlags: VTEncodeInfoFlags = []
            let frameProperties: CFDictionary? = forceNextKeyframe
                ? [kVTEncodeFrameOptionKey_ForceKeyFrame: true] as CFDictionary
                : nil
            let duration = sampleBuffer.duration.isValid
                ? sampleBuffer.duration
                : CMTime(value: 1, timescale: CMTimeScale(configuration.framesPerSecond))
            let status = VTCompressionSessionEncodeFrame(
                session,
                imageBuffer: encodingBuffer,
                presentationTimeStamp: sampleBuffer.presentationTimeStamp,
                duration: duration,
                frameProperties: frameProperties,
                sourceFrameRefcon: nil,
                infoFlagsOut: &infoFlags
            )
            guard status == noErr else {
                eventHandler(.failure("H.264 帧编码失败（\(status)）"))
                return
            }
            forceNextKeyframe = false
        } catch {
            eventHandler(.failure(error.localizedDescription))
            isAcceptingFrames = false
            invalidateCompressionSession()
        }
    }
}

nonisolated private struct EncodedFrameConfiguration: Sendable {
    let width: UInt32
    let height: UInt32
    let framesPerSecond: UInt32
    let bitrateBps: UInt32
    let streamEpoch: UInt32
    let rotation: UInt32
}

nonisolated private final class CompressionCallbackContext: @unchecked Sendable {
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
            eventHandler(.failure("VideoToolbox 输出失败（\(status)）"))
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
                width: configuration.width,
                height: configuration.height,
                framesPerSecond: configuration.framesPerSecond,
                bitrateBps: configuration.bitrateBps,
                streamEpoch: configuration.streamEpoch,
                rotation: configuration.rotation,
                parameterSets: parameterSets
            )))
        } catch {
            eventHandler(.failure(error.localizedDescription))
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
