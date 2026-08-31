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
                self.compressionContext?.updateBitrate(bitrateBps)
            } catch {
                self.eventHandler(.failure(
                    streamEpoch: self.configuration.streamEpoch,
                    encoderGeneration: self.configuration.encoderGeneration,
                    message: error.localizedDescription
                ))
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
                encoderGeneration: self.configuration.encoderGeneration,
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
            encoderGeneration: configuration.encoderGeneration,
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
                eventHandler(.failure(
                    streamEpoch: configuration.streamEpoch,
                    encoderGeneration: configuration.encoderGeneration,
                    message: "H.264 帧编码失败（\(status)）"
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
