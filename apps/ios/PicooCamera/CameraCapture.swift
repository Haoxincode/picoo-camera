import AVFoundation
import Observation
import SwiftUI
import UIKit

@MainActor
@Observable
final class CameraCaptureModel {
    private(set) var state: CameraCaptureState = .idle
    private(set) var position: CameraPosition = .back
    private(set) var resolution: VideoResolution = .p1080
    private(set) var streamEpoch: UInt32
    private(set) var encoderGeneration: UInt64
    private(set) var previewLayer: AVCaptureVideoPreviewLayer?

    let encoderEventSignals: AsyncStream<Void>

    @ObservationIgnored private let sessionReference: CaptureSessionReference
    @ObservationIgnored private let service: CameraCaptureService
    @ObservationIgnored private let encoder: VideoEncoderPipeline
    @ObservationIgnored private let encoderEventBuffer: VideoEncoderEventBuffer
    @ObservationIgnored private var targetBitrateBps: UInt32
    @ObservationIgnored private var rotationCoordinator: AVCaptureDevice.RotationCoordinator?
    @ObservationIgnored private var rotationTask: Task<Void, Never>?
    @ObservationIgnored private var operationGeneration: UInt64 = 0
    @ObservationIgnored private var startPrepared = false
    @ObservationIgnored private var orientationNotificationsActive = false

    init(initialBitrateBps: UInt32, initialStreamEpoch: UInt32) {
        let sessionReference = CaptureSessionReference()
        let eventBuffer = VideoEncoderEventBuffer()
        let initialConfiguration = VideoEncoderConfiguration(
            resolution: .p1080,
            bitrateBps: initialBitrateBps,
            streamEpoch: initialStreamEpoch,
            encoderGeneration: 1
        )
        let encoder = VideoEncoderPipeline(initialConfiguration: initialConfiguration) { event in
            eventBuffer.enqueue(event)
        }
        targetBitrateBps = initialBitrateBps
        streamEpoch = initialStreamEpoch
        encoderGeneration = 1
        self.sessionReference = sessionReference
        self.encoder = encoder
        encoderEventBuffer = eventBuffer
        encoderEventSignals = eventBuffer.signals
        service = CameraCaptureService(
            sessionReference: sessionReference,
            encoder: encoder,
            initialConfiguration: initialConfiguration
        )
    }

    func start(
        resolution requestedResolution: VideoResolution? = nil,
        bitrateBps: UInt32,
        streamEpoch: UInt32
    ) async -> Bool {
        if state == .running { return true }
        prepareForStreamingStart(streamEpoch: streamEpoch)
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
        let previousEncoderGeneration = encoderGeneration
        encoderGeneration &+= 1
        do {
            if let requestedResolution {
                resolution = requestedResolution
            }
            targetBitrateBps = bitrateBps
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
            return true
        } catch let error as CameraCaptureError {
            guard operation == operationGeneration else { return false }
            encoderGeneration = previousEncoderGeneration
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
            encoderGeneration = previousEncoderGeneration
            previewLayer = nil
            state = .failed(error.localizedDescription)
            return false
        }
    }

    func prepareForStreamingStart(streamEpoch: UInt32) {
        guard state == .idle || state == .stopping,
              !startPrepared
        else {
            return
        }
        self.streamEpoch = streamEpoch
        startPrepared = true
    }

    func rebuildAfterReconnect(streamEpoch: UInt32) async -> Bool {
        guard state == .running else { return false }
        let operation = beginOperation()
        let previousEpoch = self.streamEpoch
        let previousEncoderGeneration = encoderGeneration
        self.streamEpoch = streamEpoch
        encoderGeneration &+= 1
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
            self.streamEpoch = previousEpoch
            encoderGeneration = previousEncoderGeneration
            return false
        }
    }

    func switchCamera(streamEpoch: UInt32) async -> Bool {
        guard state == .running else { return false }
        let operation = beginOperation()
        let previousEpoch = self.streamEpoch
        let previousEncoderGeneration = encoderGeneration
        self.streamEpoch = streamEpoch
        encoderGeneration &+= 1
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
            self.streamEpoch = previousEpoch
            encoderGeneration = previousEncoderGeneration
            return false
        }
    }

    func setResolution(
        _ requestedResolution: VideoResolution,
        bitrateBps: UInt32,
        streamEpoch: UInt32
    ) async -> Bool {
        guard state == .running else { return false }
        let operation = beginOperation()
        let previousResolution = resolution
        let previousBitrate = targetBitrateBps
        let previousEpoch = self.streamEpoch
        let previousEncoderGeneration = encoderGeneration
        resolution = requestedResolution
        targetBitrateBps = bitrateBps
        self.streamEpoch = streamEpoch
        encoderGeneration &+= 1
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
            self.streamEpoch = previousEpoch
            encoderGeneration = previousEncoderGeneration
            return false
        }
    }

    /// Rebuild VideoToolbox/AVFoundation at the last Rust-committed state.
    /// Recovery is complete only after the caller observes the first matching
    /// IDR from this generation.
    func restoreCommittedConfiguration(
        resolution committedResolution: VideoResolution,
        position committedPosition: CameraPosition,
        bitrateBps committedBitrateBps: UInt32,
        streamEpoch committedStreamEpoch: UInt32
    ) async -> Bool {
        guard state == .running else { return false }
        let operation = beginOperation()
        let previousResolution = resolution
        let previousPosition = position
        let previousBitrate = targetBitrateBps
        let previousEpoch = streamEpoch
        let previousEncoderGeneration = encoderGeneration

        resolution = committedResolution
        targetBitrateBps = committedBitrateBps
        streamEpoch = committedStreamEpoch
        encoderGeneration &+= 1
        do {
            if position != committedPosition {
                let restoredPosition = try await service.switchCamera(
                    configuration: encoderConfiguration
                )
                guard operation == operationGeneration,
                      restoredPosition == committedPosition
                else {
                    throw CancellationError()
                }
                position = restoredPosition
            } else {
                try await service.setResolution(
                    committedResolution,
                    configuration: encoderConfiguration
                )
                guard operation == operationGeneration else {
                    throw CancellationError()
                }
            }
            await service.updateBitrate(committedBitrateBps)
            guard operation == operationGeneration else {
                throw CancellationError()
            }
            if let previewLayer {
                updatePreviewMirroring(previewLayer)
                startRotationUpdates(previewLayer: previewLayer)
            }
            return true
        } catch {
            guard operation == operationGeneration else { return false }
            resolution = previousResolution
            position = previousPosition
            targetBitrateBps = previousBitrate
            streamEpoch = previousEpoch
            encoderGeneration = previousEncoderGeneration
            return false
        }
    }

    func updateBitrate(_ bitrateBps: UInt32) async {
        guard bitrateBps != targetBitrateBps else { return }
        targetBitrateBps = bitrateBps
        await service.updateBitrate(bitrateBps)
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
            streamEpoch: streamEpoch,
            encoderGeneration: encoderGeneration
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
