import AVFoundation
import CoreMedia
import CoreVideo
import Foundation
import VideoToolbox

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
    private var configuration: VideoEncoderConfiguration
    private var compressionSession: VTCompressionSession?
    private var compressionContext: CompressionCallbackContext?
    private var pixelTransferSession: VTPixelTransferSession?
    private var isAcceptingFrames = false
    private var forceNextKeyframe = true

    init(
        initialConfiguration: VideoEncoderConfiguration,
        eventHandler: @escaping @Sendable (VideoEncoderEvent) -> Void
    ) {
        configuration = initialConfiguration
        self.eventHandler = eventHandler
        super.init()
    }

    func start(configuration: VideoEncoderConfiguration) async {
        await perform {
            self.isAcceptingFrames = false
            self.invalidateCompressionSession()
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
            self.configuration = VideoEncoderConfiguration(
                codec: self.configuration.codec,
                resolution: self.configuration.resolution,
                framesPerSecond: self.configuration.framesPerSecond,
                bitrateBps: bitrateBps,
                streamEpoch: self.configuration.streamEpoch,
                encoderGeneration: self.configuration.encoderGeneration,
                rotation: self.configuration.rotation
            )
            guard let session = self.compressionSession else { return }
            do {
                try Self.setBitrate(bitrateBps, on: session)
            } catch {
                self.eventHandler(.failure(
                    streamEpoch: self.configuration.streamEpoch,
                    encoderGeneration: self.configuration.encoderGeneration,
                    message: error.localizedDescription
                ))
            }
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
            codec: configuration.codec,
            width: UInt32(outputWidth),
            height: UInt32(outputHeight),
            framesPerSecond: configuration.framesPerSecond,
            bitrateBps: configuration.bitrateBps,
            streamEpoch: configuration.streamEpoch,
            encoderGeneration: configuration.encoderGeneration,
            rotation: configuration.rotation
        )
        let context = CompressionCallbackContext(
            configuration: actualConfiguration,
            eventHandler: eventHandler
        )
        let session = try Self.createPreparedSession(
            configuration: actualConfiguration,
            callback: Self.outputCallback,
            refcon: Unmanaged.passUnretained(context).toOpaque()
        )

        compressionContext = context
        compressionSession = session
        return session
    }

    /// Preparation uses the same required hardware session and properties as live encoding.
    static func canPrepare(_ source: VideoSourceFormat, bitrateBps: UInt32) -> Bool {
        let configuration = EncodedFrameConfiguration(
            codec: source.codec, width: UInt32(source.resolution.width),
            height: UInt32(source.resolution.height), framesPerSecond: source.framesPerSecond,
            bitrateBps: bitrateBps, streamEpoch: 1, encoderGeneration: 1, rotation: 0
        )
        guard let session = try? createPreparedSession(configuration: configuration,
            callback: nil, refcon: nil) else { return false }
        VTCompressionSessionInvalidate(session)
        return true
    }

    private static func createPreparedSession(
        configuration: EncodedFrameConfiguration,
        callback: VTCompressionOutputCallback?,
        refcon: UnsafeMutableRawPointer?
    ) throws -> VTCompressionSession {
        var session: VTCompressionSession?
        let encoderSpecification: CFDictionary = [
            kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder: true,
        ] as CFDictionary
        let imageBufferAttributes: CFDictionary = [
            kCVPixelBufferPixelFormatTypeKey: kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
            kCVPixelBufferWidthKey: configuration.width,
            kCVPixelBufferHeightKey: configuration.height,
            kCVPixelBufferIOSurfacePropertiesKey: [:] as CFDictionary,
        ] as CFDictionary

        let status = VTCompressionSessionCreate(
            allocator: kCFAllocatorDefault,
            width: Int32(configuration.width),
            height: Int32(configuration.height),
            codecType: configuration.codec.mediaType,
            encoderSpecification: encoderSpecification,
            imageBufferAttributes: imageBufferAttributes,
            compressedDataAllocator: nil,
            outputCallback: callback,
            refcon: refcon,
            compressionSessionOut: &session
        )
        guard status == noErr, let session else {
            throw VideoEncoderError.sessionCreation(status)
        }

        do {
            try Self.configure(session, configuration: configuration)
        } catch {
            VTCompressionSessionInvalidate(session)
            throw error
        }

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

        // Input attachments are verified before session creation and submission.
        // Explicit properties are still required: VT does not necessarily put
        // input color attachments into the encoded SPS by itself.
        try set(kVTCompressionPropertyKey_ColorPrimaries, value: kCVImageBufferColorPrimaries_ITU_R_709_2, on: session)
        try set(kVTCompressionPropertyKey_TransferFunction, value: kCVImageBufferTransferFunction_ITU_R_709_2, on: session)
        try set(kVTCompressionPropertyKey_YCbCrMatrix, value: kCVImageBufferYCbCrMatrix_ITU_R_709_2, on: session)

        // REQ-PICOO-MEDIA-026: requested profile is part of the native contract.
        try set(
            kVTCompressionPropertyKey_ProfileLevel,
            value: configuration.codec.profileLevel,
            on: session
        )
        if configuration.codec == .hevc {
            try set(kVTCompressionPropertyKey_AllowOpenGOP, value: kCFBooleanFalse, on: session)
        }
        try setBitrate(configuration.bitrateBps, on: session)

        let prepareStatus = VTCompressionSessionPrepareToEncodeFrames(session)
        guard prepareStatus == noErr else {
            throw VideoEncoderError.prepare(prepareStatus)
        }
        var hardwareValue: Unmanaged<CFTypeRef>?
        let hardwareStatus = VTSessionCopyProperty(
            session,
            key: kVTCompressionPropertyKey_UsingHardwareAcceleratedVideoEncoder,
            allocator: nil,
            valueOut: &hardwareValue
        )
        guard hardwareStatus == noErr else {
            throw VideoEncoderError.property("UsingHardwareAcceleratedVideoEncoder", hardwareStatus)
        }
        guard (hardwareValue?.takeRetainedValue() as? NSNumber)?.boolValue == true else {
            throw VideoEncoderError.hardwareEncoderUnavailable
        }
    }

    private static func validateColor(_ image: CVImageBuffer) throws {
        guard CVPixelBufferGetPixelFormatType(image) == kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange else {
            throw VideoEncoderError.sourceColorUnavailable
        }
        for (key, expected) in [
            (kCVImageBufferColorPrimariesKey, kCVImageBufferColorPrimaries_ITU_R_709_2),
            (kCVImageBufferTransferFunctionKey, kCVImageBufferTransferFunction_ITU_R_709_2),
            (kCVImageBufferYCbCrMatrixKey, kCVImageBufferYCbCrMatrix_ITU_R_709_2),
        ] {
            guard CVBufferCopyAttachment(image, key, nil) as? String == expected as String else {
                throw VideoEncoderError.sourceColorUnavailable
            }
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
        sourceFrameRefCon,
        status,
        infoFlags,
        sampleBuffer in
        guard let outputCallbackRefCon else { return }
        let context = Unmanaged<CompressionCallbackContext>
            .fromOpaque(outputCallbackRefCon)
            .takeUnretainedValue()
        context.receive(sourceFrameRefCon: sourceFrameRefCon, status: status, infoFlags: infoFlags, sampleBuffer: sampleBuffer)
    }
}

extension VideoEncoderPipeline {
    nonisolated func captureOutput(
        _ output: AVCaptureOutput,
        didOutput sampleBuffer: CMSampleBuffer,
        from connection: AVCaptureConnection
    ) {
        submit(sampleBuffer)
    }

    /// Native capture input. Calls are confined to callbackQueue.
    nonisolated func submit(_ sampleBuffer: CMSampleBuffer) {
        dispatchPrecondition(condition: .onQueue(callbackQueue))
        guard isAcceptingFrames,
              let imageBuffer = CMSampleBufferGetImageBuffer(sampleBuffer)
        else {
            return
        }

        do {
            try Self.validateColor(imageBuffer)
            let session = try compressionSession(for: imageBuffer)
            let encodingBuffer = try imageBufferForEncoding(
                imageBuffer,
                session: session
            )
            try Self.validateColor(encodingBuffer)
            var infoFlags: VTEncodeInfoFlags = []
            let frameProperties: CFDictionary? = forceNextKeyframe
                ? [kVTEncodeFrameOptionKey_ForceKeyFrame: true] as CFDictionary
                : nil
            let duration = sampleBuffer.duration.isValid
                ? sampleBuffer.duration
                : CMTime(value: 1, timescale: CMTimeScale(configuration.framesPerSecond))
            guard let context = compressionContext,
                  let identifier = context.reserveFrame(
                    bitrateBps: configuration.bitrateBps
                  )
            else { throw VideoEncoderError.pendingFramesExhausted }
            let status = VTCompressionSessionEncodeFrame(
                session,
                imageBuffer: encodingBuffer,
                presentationTimeStamp: sampleBuffer.presentationTimeStamp,
                duration: duration,
                frameProperties: frameProperties,
                sourceFrameRefcon: UnsafeMutableRawPointer(bitPattern: identifier),
                infoFlagsOut: &infoFlags
            )
            guard status == noErr else {
                // Native callbacks may run inline. Cancellation is idempotent
                // if the callback already consumed its own submission.
                context.cancelFrame(identifier)
                eventHandler(.failure(
                    streamEpoch: configuration.streamEpoch,
                    encoderGeneration: configuration.encoderGeneration,
                    message: "视频帧编码失败（\(status)）"
                ))
                return
            }
            forceNextKeyframe = false
        } catch {
            eventHandler(.failure(
                streamEpoch: configuration.streamEpoch,
                encoderGeneration: configuration.encoderGeneration,
                message: error.localizedDescription
            ))
            isAcceptingFrames = false
            invalidateCompressionSession()
        }
    }
}
