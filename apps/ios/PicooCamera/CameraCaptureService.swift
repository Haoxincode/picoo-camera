import AVFoundation
import UIKit

// REQ-PICOO-MEDIA-011: AVFoundation capture/preview -> native VideoToolbox.

nonisolated enum CameraPosition: Equatable, Sendable {
    case back
    case front

    var capturePosition: AVCaptureDevice.Position {
        switch self {
        case .back: .back
        case .front: .front
        }
    }

    var opposite: Self {
        switch self {
        case .back: .front
        case .front: .back
        }
    }
}

nonisolated enum CameraCaptureState: Equatable, Sendable {
    case idle
    case requestingPermission
    case starting
    case stopping
    case running
    case denied
    case unavailable
    case failed(String)
}

nonisolated enum CameraCaptureError: LocalizedError {
    case deviceUnavailable(CameraPosition)
    case inputUnavailable
    case inputRejected
    case outputRejected
    case frameRateUnavailable
    case deviceConfiguration

    var errorDescription: String? {
        switch self {
        case let .deviceUnavailable(position):
            return position == .front ? "前置摄像头不可用" : "后置摄像头不可用"
        case .inputUnavailable:
            return "无法创建摄像头输入"
        case .inputRejected:
            return "系统拒绝加入摄像头输入"
        case .outputRejected:
            return "系统拒绝加入视频输出"
        case .frameRateUnavailable:
            return "当前摄像头不支持请求的尺寸与帧率"
        case .deviceConfiguration:
            return "无法配置摄像头帧率"
        }
    }
}

/// Serial Swift actor boundary for every AVCaptureSession mutation.
///
/// `startRunning()` is intentionally kept off MainActor while configuration,
/// camera switching and shutdown remain serialized by actor isolation.
/// SAFETY: only `CameraCaptureService` mutates this session. MainActor merely
/// binds the same AVFoundation session to its preview layer.
nonisolated final class CaptureSessionReference: @unchecked Sendable {
    let session = AVCaptureSession()
}

actor CameraCaptureService {
    private let sessionReference: CaptureSessionReference
    private let encoder: VideoEncoderPipeline
    private var activeInput: AVCaptureDeviceInput?
    private var videoOutput: AVCaptureVideoDataOutput?
    private var position: CameraPosition = .back
    private var resolution: VideoResolution = .p1080
    private var framesPerSecond: UInt32 = 30
    private var encoderConfiguration: VideoEncoderConfiguration
    private var captureRotation: UInt32 = 0
    private var operationGeneration: UInt64 = 0

    init(
        sessionReference: CaptureSessionReference,
        encoder: VideoEncoderPipeline,
        initialConfiguration: VideoEncoderConfiguration
    ) {
        self.sessionReference = sessionReference
        self.encoder = encoder
        encoderConfiguration = initialConfiguration
    }

    func start(
        at requestedPosition: CameraPosition,
        configuration: VideoEncoderConfiguration
    ) async throws {
        let operation = beginOperation()
        let session = sessionReference.session
        do {
            try configure(
                at: requestedPosition,
                resolution: configuration.resolution,
                framesPerSecond: configuration.framesPerSecond
            )
            let appliedConfiguration = configuration.withRotation(captureRotation)
            encoderConfiguration = appliedConfiguration
            await encoder.start(configuration: appliedConfiguration)
            try ensureCurrent(operation)
            if !session.isRunning {
                session.startRunning()
            }
        } catch {
            if operation == operationGeneration {
                await encoder.stop()
                if operation == operationGeneration, session.isRunning {
                    session.stopRunning()
                }
            }
            throw error
        }
    }

    func switchCamera(
        configuration: VideoEncoderConfiguration
    ) async throws -> CameraPosition {
        let operation = beginOperation()
        let target = position.opposite
        return try await reconfigureCapture(
            at: target,
            configuration: configuration,
            operation: operation
        )
    }

    func setResolution(
        _ resolution: VideoResolution,
        configuration: VideoEncoderConfiguration
    ) async throws {
        let operation = beginOperation()
        _ = try await reconfigureCapture(
            at: position,
            configuration: configuration,
            operation: operation
        )
    }

    func updateBitrate(_ bitrateBps: UInt32) async {
        encoderConfiguration = VideoEncoderConfiguration(
            resolution: encoderConfiguration.resolution,
            framesPerSecond: encoderConfiguration.framesPerSecond,
            bitrateBps: bitrateBps,
            streamEpoch: encoderConfiguration.streamEpoch,
            encoderGeneration: encoderConfiguration.encoderGeneration,
            rotation: encoderConfiguration.rotation
        )
        await encoder.updateBitrate(bitrateBps)
    }

    func updateRotation(_ rotation: UInt32) async {
        captureRotation = rotation % 360
        encoderConfiguration = encoderConfiguration.withRotation(captureRotation)
        await encoder.updateRotation(captureRotation)
    }

    func requestKeyframe() async {
        await encoder.requestKeyframe()
    }

    func stop() async {
        let operation = beginOperation()
        await encoder.stop()
        guard operation == operationGeneration else { return }
        let session = sessionReference.session
        if session.isRunning {
            session.stopRunning()
        }
    }

    private func reconfigureCapture(
        at requestedPosition: CameraPosition,
        configuration: VideoEncoderConfiguration,
        operation: UInt64
    ) async throws -> CameraPosition {
        await encoder.pause()
        try ensureCurrent(operation)
        do {
            try configure(
                at: requestedPosition,
                resolution: configuration.resolution,
                framesPerSecond: configuration.framesPerSecond
            )
            let appliedConfiguration = configuration.withRotation(captureRotation)
            encoderConfiguration = appliedConfiguration
            await encoder.start(configuration: appliedConfiguration)
            try ensureCurrent(operation)
            return requestedPosition
        } catch {
            if error is CancellationError { throw error }
            try ensureCurrent(operation)
            let rollbackConfiguration = encoderConfiguration
            await encoder.start(configuration: rollbackConfiguration)
            try ensureCurrent(operation)
            throw error
        }
    }

    private func configure(
        at requestedPosition: CameraPosition,
        resolution requestedResolution: VideoResolution,
        framesPerSecond requestedFps: UInt32
    ) throws {
        let session = sessionReference.session
        if activeInput != nil,
           position == requestedPosition,
           resolution == requestedResolution,
           framesPerSecond == requestedFps,
           videoOutput != nil {
            return
        }

        let replacingInput = activeInput == nil || position != requestedPosition
        let input = try replacingInput
            ? makeInput(at: requestedPosition)
            : activeInput
        let previousInput = activeInput
        let previousOutput = videoOutput
        guard let device = input?.device else { throw CameraCaptureError.inputUnavailable }
        let format = try Self.captureFormat(
            from: device.formats, resolution: requestedResolution, framesPerSecond: requestedFps
        )
        // Every throwing capability/lock check precedes source format mutation.
        do { try device.lockForConfiguration() }
        catch { throw CameraCaptureError.deviceConfiguration }
        defer { device.unlockForConfiguration() }

        session.beginConfiguration()
        do {
            if replacingInput, let input {
                if let activeInput {
                    session.removeInput(activeInput)
                }
                guard session.canAddInput(input) else {
                    throw CameraCaptureError.inputRejected
                }
                session.addInput(input)
                activeInput = input
            }

            if videoOutput == nil {
                let output = AVCaptureVideoDataOutput()
                output.alwaysDiscardsLateVideoFrames = true
                output.videoSettings = [
                    kCVPixelBufferPixelFormatTypeKey as String:
                        kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
                ]
                output.setSampleBufferDelegate(encoder, queue: encoder.callbackQueue)
                guard session.canAddOutput(output) else {
                    throw CameraCaptureError.outputRejected
                }
                session.addOutput(output)
                videoOutput = output
            }

            if let connection = videoOutput?.connection(with: .video),
               connection.isVideoRotationAngleSupported(0) {
                // Camera buffer defaults differ across iPad generations. Keep the
                // encoded buffer in native sensor orientation and carry rotation
                // explicitly in StreamConfig.
                connection.videoRotationAngle = 0
            }

            // Setting activeFormat selects inputPriority. A session preset would
            // let AVFoundation choose another format and reset frame durations.
            device.activeFormat = format
            if format.isAutoVideoFrameRateSupported {
                device.isAutoVideoFrameRateEnabled = false
            }
            let duration = CMTime(value: 1, timescale: CMTimeScale(requestedFps))
            device.activeVideoMinFrameDuration = duration
            device.activeVideoMaxFrameDuration = duration

            position = requestedPosition
            resolution = requestedResolution
            framesPerSecond = requestedFps
            if let device = activeInput?.device {
                let coordinator = AVCaptureDevice.RotationCoordinator(
                    device: device,
                    previewLayer: nil
                )
                captureRotation = UInt32(
                    coordinator.videoRotationAngleForHorizonLevelCapture.rounded()
                ) % 360
            }
            session.commitConfiguration()
        } catch {
            let inputWasReplaced = activeInput !== previousInput
            if inputWasReplaced, let currentInput = activeInput {
                session.removeInput(currentInput)
            }
            if let currentOutput = videoOutput,
               currentOutput !== previousOutput {
                session.removeOutput(currentOutput)
            }
            if let previousInput,
               !session.inputs.contains(where: { $0 === previousInput }) {
                if session.canAddInput(previousInput) {
                    session.addInput(previousInput)
                    activeInput = previousInput
                } else {
                    activeInput = nil
                }
            } else {
                activeInput = previousInput
            }
            videoOutput = previousOutput
            session.commitConfiguration()
            throw error
        }
    }

    private func makeInput(
        at requestedPosition: CameraPosition
    ) throws -> AVCaptureDeviceInput {
        guard let device = AVCaptureDevice.default(
            .builtInWideAngleCamera,
            for: .video,
            position: requestedPosition.capturePosition
        ) else {
            throw CameraCaptureError.deviceUnavailable(requestedPosition)
        }
        do {
            return try AVCaptureDeviceInput(device: device)
        } catch {
            throw CameraCaptureError.inputUnavailable
        }
    }

    // REQ-PICOO-MEDIA-040: select an actual combined format/rate, never
    // independently combine a device's maximum dimensions and maximum fps.
    static func captureFormat(
        from formats: [AVCaptureDevice.Format],
        resolution: VideoResolution,
        framesPerSecond: UInt32
    ) throws -> AVCaptureDevice.Format {
        guard framesPerSecond == 30 || framesPerSecond == 60 else {
            throw CameraCaptureError.frameRateUnavailable
        }
        let fps = Double(framesPerSecond)
        guard let format = formats.first(where: { format in
            let dimensions = CMVideoFormatDescriptionGetDimensions(format.formatDescription)
            let pixelFormat = CMFormatDescriptionGetMediaSubType(format.formatDescription)
            return dimensions.width == resolution.width && dimensions.height == resolution.height
                && (pixelFormat == kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
                    || pixelFormat == kCVPixelFormatType_420YpCbCr8BiPlanarFullRange)
                && format.videoSupportedFrameRateRanges.contains(where: {
                    $0.minFrameRate <= fps && $0.maxFrameRate >= fps
                })
        }) else { throw CameraCaptureError.frameRateUnavailable }
        return format
    }

    private func beginOperation() -> UInt64 {
        operationGeneration &+= 1
        return operationGeneration
    }

    private func ensureCurrent(_ operation: UInt64) throws {
        guard operation == operationGeneration else { throw CancellationError() }
    }
}

nonisolated private extension VideoEncoderConfiguration {
    func withRotation(_ rotation: UInt32) -> Self {
        Self(
            resolution: resolution,
            framesPerSecond: framesPerSecond,
            bitrateBps: bitrateBps,
            streamEpoch: streamEpoch,
            encoderGeneration: encoderGeneration,
            rotation: rotation
        )
    }
}

