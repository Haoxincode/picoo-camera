import AVFoundation
import CoreMedia
import CoreVideo
import Foundation
import VideoToolbox

// REQ-PICOO-MEDIA-011: native 420v capture -> hardware H.264 Access Units.

nonisolated enum VideoResolution: Int, CaseIterable, Hashable, Sendable {
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

nonisolated struct VideoEncoderConfiguration: Equatable, Sendable {
    let resolution: VideoResolution
    let framesPerSecond: UInt32
    let bitrateBps: UInt32
    let streamEpoch: UInt32
    let encoderGeneration: UInt64
    let rotation: UInt32

    init(
        resolution: VideoResolution,
        framesPerSecond: UInt32 = 30,
        bitrateBps: UInt32,
        streamEpoch: UInt32,
        encoderGeneration: UInt64,
        rotation: UInt32 = 0
    ) {
        self.resolution = resolution
        self.framesPerSecond = framesPerSecond
        self.bitrateBps = bitrateBps
        self.streamEpoch = streamEpoch
        self.encoderGeneration = encoderGeneration
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
    let encoderGeneration: UInt64
    let rotation: UInt32
    let parameterSets: H264ParameterSets?
}

nonisolated enum VideoEncoderEvent: Sendable {
    case accessUnit(EncodedAccessUnit)
    case failure(streamEpoch: UInt32, encoderGeneration: UInt64, message: String)
    case queueOverflow(streamEpoch: UInt32, encoderGeneration: UInt64)

    var streamEpoch: UInt32 {
        switch self {
        case let .accessUnit(accessUnit): accessUnit.streamEpoch
        case let .failure(streamEpoch, _, _), let .queueOverflow(streamEpoch, _): streamEpoch
        }
    }

    var encoderGeneration: UInt64 {
        switch self {
        case let .accessUnit(accessUnit): accessUnit.encoderGeneration
        case let .failure(_, generation, _), let .queueOverflow(_, generation): generation
        }
    }
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
                    events.append(.queueOverflow(
                        streamEpoch: accessUnit.streamEpoch,
                        encoderGeneration: accessUnit.encoderGeneration
                    ))
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
