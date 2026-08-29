import AVFoundation
import Observation
import SwiftUI
import UIKit

// REQ-PICOO-MEDIA-011 (capture/preview boundary; VideoToolbox follows separately).

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
    case running
    case denied
    case unavailable
    case failed(String)
}

nonisolated enum CameraCaptureError: LocalizedError {
    case deviceUnavailable(CameraPosition)
    case inputUnavailable
    case inputRejected

    var errorDescription: String? {
        switch self {
        case let .deviceUnavailable(position):
            return position == .front ? "前置摄像头不可用" : "后置摄像头不可用"
        case .inputUnavailable:
            return "无法创建摄像头输入"
        case .inputRejected:
            return "系统拒绝加入摄像头输入"
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
    private var activeInput: AVCaptureDeviceInput?
    private var position: CameraPosition = .back

    fileprivate init(sessionReference: CaptureSessionReference) {
        self.sessionReference = sessionReference
    }

    func start(at requestedPosition: CameraPosition) throws {
        let session = sessionReference.session
        try configure(at: requestedPosition)
        if !session.isRunning {
            session.startRunning()
        }
    }

    func switchCamera() throws -> CameraPosition {
        let target = position.opposite
        try configure(at: target)
        return target
    }

    func stop() {
        let session = sessionReference.session
        if session.isRunning {
            session.stopRunning()
        }
    }

    private func configure(at requestedPosition: CameraPosition) throws {
        let session = sessionReference.session
        if activeInput != nil, position == requestedPosition {
            return
        }

        guard let device = AVCaptureDevice.default(
            .builtInWideAngleCamera,
            for: .video,
            position: requestedPosition.capturePosition
        ) else {
            throw CameraCaptureError.deviceUnavailable(requestedPosition)
        }

        let input: AVCaptureDeviceInput
        do {
            input = try AVCaptureDeviceInput(device: device)
        } catch {
            throw CameraCaptureError.inputUnavailable
        }

        session.beginConfiguration()
        defer { session.commitConfiguration() }

        if session.canSetSessionPreset(.hd1920x1080) {
            session.sessionPreset = .hd1920x1080
        } else if session.canSetSessionPreset(.hd1280x720) {
            session.sessionPreset = .hd1280x720
        }

        if let activeInput {
            session.removeInput(activeInput)
        }

        guard session.canAddInput(input) else {
            if let activeInput, session.canAddInput(activeInput) {
                session.addInput(activeInput)
            }
            throw CameraCaptureError.inputRejected
        }

        session.addInput(input)
        activeInput = input
        position = requestedPosition
    }
}

@MainActor
@Observable
final class CameraCaptureModel {
    private(set) var state: CameraCaptureState = .idle
    private(set) var position: CameraPosition = .back
    private(set) var previewLayer: AVCaptureVideoPreviewLayer?

    @ObservationIgnored private let sessionReference: CaptureSessionReference
    @ObservationIgnored private let service: CameraCaptureService

    init() {
        let sessionReference = CaptureSessionReference()
        self.sessionReference = sessionReference
        service = CameraCaptureService(sessionReference: sessionReference)
    }

    func start() async -> Bool {
        let authorized: Bool
        switch AVCaptureDevice.authorizationStatus(for: .video) {
        case .authorized:
            authorized = true
        case .notDetermined:
            state = .requestingPermission
            authorized = await AVCaptureDevice.requestAccess(for: .video)
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
            try await service.start(at: position)
            let previewLayer = AVCaptureVideoPreviewLayer(session: sessionReference.session)
            previewLayer.videoGravity = .resizeAspectFill
            self.previewLayer = previewLayer
            state = .running
            return true
        } catch let error as CameraCaptureError {
            previewLayer = nil
            switch error {
            case .deviceUnavailable:
                state = .unavailable
            default:
                state = .failed(error.localizedDescription)
            }
            return false
        } catch {
            previewLayer = nil
            state = .failed(error.localizedDescription)
            return false
        }
    }

    func switchCamera() async {
        guard state == .running else { return }
        do {
            position = try await service.switchCamera()
        } catch {
            state = .failed(error.localizedDescription)
        }
    }

    func stop() async {
        await service.stop()
        previewLayer = nil
        state = .idle
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
