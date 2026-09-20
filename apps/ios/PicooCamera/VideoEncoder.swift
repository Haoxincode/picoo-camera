import AVFoundation
import CoreMedia
import CoreVideo
import Foundation
import VideoToolbox

// REQ-PICOO-MEDIA-011: native 420v capture -> hardware AVC/HEVC Access Units.

nonisolated enum VideoResolution: Int, CaseIterable, Hashable, Sendable {
    case p720 = 720
    case p1080 = 1080

    var width: Int32 {
        switch self {
        case .p720: 1280
        case .p1080: 1920
        }
    }

    var height: Int32 { Int32(rawValue) }

    // REQ-PICOO-MEDIA-028: exact admission, no implicit source-size substitution.
    static func supported(forRequestedHeight height: UInt32) -> Self? {
        Self(rawValue: Int(height))
    }

}

nonisolated enum NativeVideoCodec: UInt32, CaseIterable, Hashable, Sendable {
    case avc = 1
    case hevc = 2

    var mediaType: CMVideoCodecType {
        switch self { case .avc: kCMVideoCodecType_H264; case .hevc: kCMVideoCodecType_HEVC }
    }

    var profileLevel: CFString {
        switch self {
        case .avc: kVTProfileLevel_H264_High_AutoLevel
        case .hevc: kVTProfileLevel_HEVC_Main_AutoLevel
        }
    }
}

nonisolated struct VideoEncoderConfiguration: Equatable, Sendable {
    let codec: NativeVideoCodec
    let resolution: VideoResolution
    let framesPerSecond: UInt32
    let bitrateBps: UInt32
    let streamEpoch: UInt32
    let encoderGeneration: UInt64
    let rotation: UInt32

    init(
        codec: NativeVideoCodec,
        resolution: VideoResolution,
        framesPerSecond: UInt32,
        bitrateBps: UInt32,
        streamEpoch: UInt32,
        encoderGeneration: UInt64,
        rotation: UInt32 = 0
    ) {
        self.codec = codec
        self.resolution = resolution
        self.framesPerSecond = framesPerSecond
        self.bitrateBps = bitrateBps
        self.streamEpoch = streamEpoch
        self.encoderGeneration = encoderGeneration
        self.rotation = rotation % 360
    }
}

nonisolated struct EncodedCodecConfiguration: Equatable, Sendable {
    let codec: UInt32
    let record: Data
}

nonisolated struct EncodedAccessUnit: Equatable, Sendable {
    let data: Data
    let isKeyframe: Bool
    let presentationTimeUs: UInt64
    let encodedAtUs: UInt64
    let width: UInt32
    let height: UInt32
    let framesPerSecond: UInt32
    let bitrateBps: UInt32
    let streamEpoch: UInt32
    let encoderGeneration: UInt64
    let rotation: UInt32
    let codecConfiguration: EncodedCodecConfiguration
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
/// IDR arrives. Only wake-up tokens use AsyncStream; compressed Data stays bounded.
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
    case sourceColorUnavailable
    case pendingFramesExhausted
    case sessionCreation(OSStatus)
    case hardwareEncoderUnavailable
    case property(String, OSStatus)
    case prepare(OSStatus)
    case sourceDimensionsMismatch

    var errorDescription: String? {
        switch self {
        case .sourceColorUnavailable:
            "采集图像缺少已确认的 BT.709 SDR 描述"
        case .pendingFramesExhausted:
            "编码器待完成帧已达到容量上限"
        case .hardwareEncoderUnavailable:
            "没有可用的请求格式的硬件视频编码器"
        case let .sessionCreation(status):
            "无法创建硬件视频编码器（\(status)）"
        case let .property(key, status):
            "无法配置 视频编码参数 \(key)（\(status)）"
        case let .prepare(status):
            "视频编码器准备失败（\(status)）"
        case .sourceDimensionsMismatch:
            "采集图像尺寸与准备的视频配置不一致"
        }
    }
}
