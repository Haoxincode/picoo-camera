import AVFoundation
import Observation
import SwiftUI
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
            return "当前摄像头不支持 30 FPS"
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
nonisolated private final class CaptureSessionReference: @unchecked Sendable {
    let session = AVCaptureSession()
}

actor CameraCaptureService {
    private let sessionReference: CaptureSessionReference
    private let encoder: VideoEncoderPipeline
    private var activeInput: AVCaptureDeviceInput?
    private var videoOutput: AVCaptureVideoDataOutput?
    private var position: CameraPosition = .back
    private var resolution: VideoResolution = .p1080
    private var encoderConfiguration = VideoEncoderConfiguration(resolution: .p1080)
    private var captureRotation: UInt32 = 0
    private var operationGeneration: UInt64 = 0

    fileprivate init(
        sessionReference: CaptureSessionReference,
        encoder: VideoEncoderPipeline
    ) {
        self.sessionReference = sessionReference
        self.encoder = encoder
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
                resolution: configuration.resolution
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
                resolution: configuration.resolution
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
        resolution requestedResolution: VideoResolution
    ) throws {
        let session = sessionReference.session
        if activeInput != nil,
           position == requestedPosition,
           resolution == requestedResolution,
           videoOutput != nil {
            return
        }

        let replacingInput = activeInput == nil || position != requestedPosition
        let input = try replacingInput
            ? makeInput(at: requestedPosition)
            : activeInput
        let previousInput = activeInput
        let previousOutput = videoOutput
        let previousPreset = session.sessionPreset

        session.beginConfiguration()
        do {
            let preset: AVCaptureSession.Preset = requestedResolution == .p1080
                ? .hd1920x1080
                : .hd1280x720
            guard session.canSetSessionPreset(preset) else {
                throw CameraCaptureError.deviceUnavailable(requestedPosition)
            }
            session.sessionPreset = preset

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

            if let device = activeInput?.device {
                try configureThirtyFramesPerSecond(on: device)
            }

            position = requestedPosition
            resolution = requestedResolution
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
            if session.canSetSessionPreset(previousPreset) {
                session.sessionPreset = previousPreset
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

    private func configureThirtyFramesPerSecond(on device: AVCaptureDevice) throws {
        let framesPerSecond = 30.0
        guard device.activeFormat.videoSupportedFrameRateRanges.contains(where: {
            $0.minFrameRate <= framesPerSecond && $0.maxFrameRate >= framesPerSecond
        }) else {
            throw CameraCaptureError.frameRateUnavailable
        }
        do {
            try device.lockForConfiguration()
            defer { device.unlockForConfiguration() }
            let frameDuration = CMTime(value: 1, timescale: 30)
            device.activeVideoMinFrameDuration = frameDuration
            device.activeVideoMaxFrameDuration = frameDuration
        } catch {
            throw CameraCaptureError.deviceConfiguration
        }
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
            rotation: rotation
        )
    }
}

@MainActor
@Observable
final class CameraCaptureModel {
    private(set) var state: CameraCaptureState = .idle
    private(set) var position: CameraPosition = .back
    private(set) var resolution: VideoResolution = .p1080
    private(set) var streamEpoch = StreamEpoch.initial
    private(set) var previewLayer: AVCaptureVideoPreviewLayer?

    let encoderEventSignals: AsyncStream<Void>

    @ObservationIgnored private let sessionReference: CaptureSessionReference
    @ObservationIgnored private let service: CameraCaptureService
    @ObservationIgnored private let encoder: VideoEncoderPipeline
    @ObservationIgnored private let encoderEventBuffer: VideoEncoderEventBuffer
    @ObservationIgnored private var targetBitrateBps = VideoBitrate.initial(for: .p1080)
    @ObservationIgnored private var rotationCoordinator: AVCaptureDevice.RotationCoordinator?
    @ObservationIgnored private var rotationTask: Task<Void, Never>?
    @ObservationIgnored private var operationGeneration: UInt64 = 0
    @ObservationIgnored private var hasRunEncoder = false
    @ObservationIgnored private var startPrepared = false
    @ObservationIgnored private var orientationNotificationsActive = false

    init() {
        let sessionReference = CaptureSessionReference()
        let eventBuffer = VideoEncoderEventBuffer()
        let encoder = VideoEncoderPipeline { event in
            eventBuffer.enqueue(event)
        }
        self.sessionReference = sessionReference
        self.encoder = encoder
        encoderEventBuffer = eventBuffer
        encoderEventSignals = eventBuffer.signals
        service = CameraCaptureService(
            sessionReference: sessionReference,
            encoder: encoder
        )
    }

    func start(resolution requestedResolution: VideoResolution? = nil) async -> Bool {
        if state == .running { return true }
        prepareForStreamingStart()
        startPrepared = false
        let operation = beginOperation()
        let authorized: Bool
        switch AVCaptureDevice.authorizationStatus(for: .video) {
        case .authorized:
            authorized = true
        case .notDetermined:
            state = .requestingPermission
            authorized = await AVCaptureDevice.requestAccess(for: .video)
            guard operation == operationGeneration else { return false }
        case .denied, .restricted:
            authorized = false
        @unknown default:
            authorized = false
        }

        guard authorized else {
            state = .denied
            previewLayer = nil
            return false
        }

        state = .starting
        do {
            if let requestedResolution {
                resolution = requestedResolution
                targetBitrateBps = VideoBitrate.initial(for: requestedResolution)
            }
            try await service.start(
                at: position,
                configuration: encoderConfiguration
            )
            guard operation == operationGeneration else { return false }
            let previewLayer = AVCaptureVideoPreviewLayer(session: sessionReference.session)
            previewLayer.videoGravity = .resizeAspectFill
            updatePreviewMirroring(previewLayer)
            self.previewLayer = previewLayer
            startRotationUpdates(previewLayer: previewLayer)
            state = .running
            hasRunEncoder = true
            return true
        } catch let error as CameraCaptureError {
            guard operation == operationGeneration else { return false }
            previewLayer = nil
            switch error {
            case .deviceUnavailable:
                state = .unavailable
            default:
                state = .failed(error.localizedDescription)
            }
            return false
        } catch {
            guard operation == operationGeneration else { return false }
            previewLayer = nil
            state = .failed(error.localizedDescription)
            return false
        }
    }

    func prepareForStreamingStart() {
        guard state == .idle || state == .stopping,
              !startPrepared
        else {
            return
        }
        if hasRunEncoder {
            streamEpoch = StreamEpoch.bump(streamEpoch)
        }
        startPrepared = true
    }

    func rebuildAfterReconnect() async -> Bool {
        guard state == .running else { return false }
        let operation = beginOperation()
        let previousEpoch = streamEpoch
        streamEpoch = StreamEpoch.bump(streamEpoch)
        do {
            try await service.setResolution(
                resolution,
                configuration: encoderConfiguration
            )
            guard operation == operationGeneration else { return false }
            await service.updateBitrate(targetBitrateBps)
            guard operation == operationGeneration else { return false }
            return true
        } catch {
            guard operation == operationGeneration else { return false }
            streamEpoch = previousEpoch
            return false
        }
    }

    func switchCamera() async -> Bool {
        guard state == .running else { return false }
        let operation = beginOperation()
        let previousEpoch = streamEpoch
        streamEpoch = StreamEpoch.bump(streamEpoch)
        do {
            let switchedPosition = try await service.switchCamera(
                configuration: encoderConfiguration
            )
            guard operation == operationGeneration else { return false }
            await service.updateBitrate(targetBitrateBps)
            guard operation == operationGeneration else { return false }
            position = switchedPosition
            if let previewLayer {
                updatePreviewMirroring(previewLayer)
                startRotationUpdates(previewLayer: previewLayer)
            }
            return true
        } catch {
            guard operation == operationGeneration else { return false }
            streamEpoch = previousEpoch
            return false
        }
    }

    func setResolution(_ requestedResolution: VideoResolution) async -> Bool {
        guard state == .running else { return false }
        guard resolution != requestedResolution else { return true }
        let operation = beginOperation()
        let previousResolution = resolution
        let previousBitrate = targetBitrateBps
        let previousEpoch = streamEpoch
        resolution = requestedResolution
        targetBitrateBps = VideoBitrate.initial(for: requestedResolution)
        streamEpoch = StreamEpoch.bump(streamEpoch)
        do {
            try await service.setResolution(
                requestedResolution,
                configuration: encoderConfiguration
            )
            guard operation == operationGeneration else { return false }
            await service.updateBitrate(targetBitrateBps)
            guard operation == operationGeneration else { return false }
            return true
        } catch {
            guard operation == operationGeneration else { return false }
            resolution = previousResolution
            targetBitrateBps = previousBitrate
            streamEpoch = previousEpoch
            return false
        }
    }

    func updateBitrate(_ bitrateBps: UInt32) async {
        let clamped = VideoBitrate.clamp(bitrateBps, for: resolution)
        guard clamped != targetBitrateBps else { return }
        targetBitrateBps = clamped
        await service.updateBitrate(clamped)
    }

    func requestKeyframe() async {
        await service.requestKeyframe()
    }

    nonisolated func drainEncoderEvents() -> [VideoEncoderEvent] {
        encoderEventBuffer.drain()
    }

    nonisolated func discardEncoderEvents() {
        _ = encoderEventBuffer.drain()
    }

    nonisolated func discardEncoderEventsUntilKeyframe() {
        encoderEventBuffer.discardUntilKeyframe()
    }

    func stop() async {
        let operation = beginOperation()
        state = .stopping
        stopRotationUpdates()
        await service.stop()
        guard operation == operationGeneration else { return }
        previewLayer = nil
        state = .idle
    }

    private var encoderConfiguration: VideoEncoderConfiguration {
        VideoEncoderConfiguration(
            resolution: resolution,
            bitrateBps: targetBitrateBps,
            streamEpoch: streamEpoch
        )
    }

    private func updatePreviewMirroring(_ previewLayer: AVCaptureVideoPreviewLayer) {
        guard let connection = previewLayer.connection else { return }
        connection.automaticallyAdjustsVideoMirroring = false
        connection.isVideoMirrored = position == .front
    }

    private func startRotationUpdates(previewLayer: AVCaptureVideoPreviewLayer) {
        stopRotationUpdates()
        guard let device = AVCaptureDevice.default(
            .builtInWideAngleCamera,
            for: .video,
            position: position.capturePosition
        ) else {
            return
        }
        let coordinator = AVCaptureDevice.RotationCoordinator(
            device: device,
            previewLayer: previewLayer
        )
        rotationCoordinator = coordinator
        UIDevice.current.beginGeneratingDeviceOrientationNotifications()
        orientationNotificationsActive = true
        applyRotation(from: coordinator, to: previewLayer)
        rotationTask = Task { [weak self, weak previewLayer] in
            for await _ in NotificationCenter.default.notifications(
                named: UIDevice.orientationDidChangeNotification
            ) {
                guard !Task.isCancelled,
                      let self,
                      let previewLayer,
                      let coordinator = self.rotationCoordinator
                else {
                    return
                }
                self.applyRotation(from: coordinator, to: previewLayer)
            }
        }
    }

    private func stopRotationUpdates() {
        rotationTask?.cancel()
        rotationTask = nil
        rotationCoordinator = nil
        if orientationNotificationsActive {
            UIDevice.current.endGeneratingDeviceOrientationNotifications()
            orientationNotificationsActive = false
        }
    }

    private func applyRotation(
        from coordinator: AVCaptureDevice.RotationCoordinator,
        to previewLayer: AVCaptureVideoPreviewLayer
    ) {
        let previewAngle = coordinator.videoRotationAngleForHorizonLevelPreview
        if let connection = previewLayer.connection,
           connection.isVideoRotationAngleSupported(previewAngle) {
            connection.videoRotationAngle = previewAngle
        }
        let captureAngle = UInt32(
            coordinator.videoRotationAngleForHorizonLevelCapture.rounded()
        ) % 360
        Task { await service.updateRotation(captureAngle) }
    }

    private func beginOperation() -> UInt64 {
        operationGeneration &+= 1
        return operationGeneration
    }
}

struct CameraPreview: UIViewRepresentable {
    let previewLayer: AVCaptureVideoPreviewLayer

    func makeUIView(context: Context) -> CameraPreviewHostView {
        CameraPreviewHostView(previewLayer: previewLayer)
    }

    func updateUIView(_ view: CameraPreviewHostView, context: Context) {
        view.setPreviewLayer(previewLayer)
    }
}

final class CameraPreviewHostView: UIView {
    private var activePreviewLayer: AVCaptureVideoPreviewLayer

    init(previewLayer: AVCaptureVideoPreviewLayer) {
        activePreviewLayer = previewLayer
        super.init(frame: .zero)
        layer.addSublayer(previewLayer)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) is unavailable")
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        activePreviewLayer.frame = bounds
    }

    func setPreviewLayer(_ previewLayer: AVCaptureVideoPreviewLayer) {
        guard activePreviewLayer !== previewLayer else { return }
        activePreviewLayer.removeFromSuperlayer()
        activePreviewLayer = previewLayer
        layer.addSublayer(previewLayer)
        setNeedsLayout()
    }
}
