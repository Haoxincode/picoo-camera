import CoreMedia
import CoreMediaIO
import CoreVideo
import Foundation
import IOKit.audio
import os

private let ringIdentityProbeIntervalNanoseconds: UInt64 = NSEC_PER_SEC

private struct PreparedFrameKey: Equatable {
    let ringGeneration: UInt64
    let contentGeneration: UInt64
    let sequence: UInt64
    let formatIndex: Int
}

private struct OutputFormat {
    let width: Int
    let height: Int
    let frameRate: UInt64
    let description: CMFormatDescription
    let streamFormat: CMIOExtensionStreamFormat
    let pool: OutputPixelBufferPool
}

final class PicooCameraDeviceSource: NSObject, CMIOExtensionDeviceSource, @unchecked Sendable {
    private(set) var device: CMIOExtensionDevice!

    private let frameQueue = DispatchQueue(
        label: "com.haoxincode.picoo-camera.extension.frames",
        qos: .userInteractive,
        autoreleaseFrequency: .workItem
    )
    private var timer: DispatchSourceTimer?
    private var sampleClock = OutputSampleClock()
    private var streamingClients: UInt32 = 0
    private var ringReader: SharedRingReader?
    private var ringGeneration: UInt64 = 0
    private var lastRingIdentityProbeAt: UInt64 = 0
    private var sampleWasDropped = false
    private var preparedFrameKey: PreparedFrameKey?
    private var preparedPixelBuffer: CVPixelBuffer?
    private let scaleWorkspace = VImageScaleWorkspace()
    private let outputFormats: [OutputFormat]
    private var streamSource: PicooCameraStreamSource!

    init(localizedName: String) throws {
        outputFormats = try [(1280, 720), (1920, 1080)].flatMap { width, height in
            let pool = try OutputPixelBufferPool(width: width, height: height)
            return try [UInt64(30), UInt64(60)].map { frameRate in
                try Self.makeOutputFormat(width: width, height: height, frameRate: frameRate, pool: pool)
            }
        }
        super.init()

        guard let deviceID = UUID(uuidString: "9D0187D2-9E73-4A49-8F0D-5049434F4F01"),
              let streamID = UUID(uuidString: "9D0187D2-9E73-4A49-8F0D-5049434F4F02")
        else {
            throw CocoaError(.coderInvalidValue)
        }
        device = CMIOExtensionDevice(
            localizedName: localizedName,
            deviceID: deviceID,
            legacyDeviceID: "com.haoxincode.picoo-camera.virtual-camera",
            source: self
        )
        streamSource = PicooCameraStreamSource(
            localizedName: "Picoo Camera Video",
            streamID: streamID,
            formats: outputFormats.map(\.streamFormat),
            device: device
        )
        try device.addStream(streamSource.stream)
    }

    var availableProperties: Set<CMIOExtensionProperty> {
        [.deviceTransportType, .deviceModel]
    }

    func deviceProperties(
        forProperties properties: Set<CMIOExtensionProperty>
    ) throws -> CMIOExtensionDeviceProperties {
        let result = CMIOExtensionDeviceProperties(dictionary: [:])
        if properties.contains(.deviceTransportType) {
            result.transportType = kIOAudioDeviceTransportTypeVirtual
        }
        if properties.contains(.deviceModel) {
            result.model = "Picoo Camera"
        }
        return result
    }

    func setDeviceProperties(_ deviceProperties: CMIOExtensionDeviceProperties) throws {}

    func startStreaming() {
        frameQueue.sync {
            streamingClients += 1
            guard streamingClients == 1 else { return }

            let timer = DispatchSource.makeTimerSource(flags: .strict, queue: frameQueue)
            sampleClock = OutputSampleClock()
            sampleWasDropped = false
            timer.schedule(deadline: .now(), leeway: .milliseconds(1))
            timer.setEventHandler { [weak self] in
                self?.emitFrame()
            }
            self.timer = timer
            timer.resume()
        }
    }

    func stopStreaming() {
        frameQueue.sync {
            guard streamingClients > 0 else { return }
            streamingClients -= 1
            guard streamingClients == 0 else { return }
            ringReader?.clearCpuDemand()
            timer?.cancel()
            timer = nil
            ringReader = nil
            preparedFrameKey = nil
            preparedPixelBuffer = nil
        }
    }

    private func emitFrame() {
        let formatIndex = streamSource.activeFormatIndex
        guard outputFormats.indices.contains(formatIndex) else { return }
        let format = outputFormats[formatIndex]

        let now = DispatchTime.now().uptimeNanoseconds
        let tick: OutputSampleClock.Tick
        do {
            guard let due = try sampleClock.tick(now: now, frameRate: format.frameRate) else {
                scheduleNextSample()
                return
            }
            tick = due
        } catch {
            Logger.extension.error("Output sample clock failed: \(error)")
            timer?.cancel()
            timer = nil
            return
        }
        var emitted = false
        defer {
            if !emitted { sampleWasDropped = true }
            scheduleNextSample()
        }
        if ringReader != nil,
           now &- lastRingIdentityProbeAt >= ringIdentityProbeIntervalNanoseconds
        {
            lastRingIdentityProbeAt = now
            if ringReader?.stillMapsCurrentFile() == false {
                resetRingReader()
            }
        }
        if ringReader == nil {
            if let opened = try? SharedRingReader.openAppGroupRing() {
                ringReader = opened
                ringGeneration &+= 1
                lastRingIdentityProbeAt = now
            }
        }
        let acquired = ringReader?.acquireLatestFrame()
        let frame = acquired?.isCurrent == true ? acquired : nil
        let contentGeneration = ringReader?.contentGeneration ?? 0
        if frame == nil, ringReader?.stillMapsCurrentFile() == false {
            resetRingReader()
        }

        let key = frame.map {
            PreparedFrameKey(
                ringGeneration: ringGeneration,
                contentGeneration: $0.contentGeneration,
                sequence: $0.sequence,
                formatIndex: formatIndex
            )
        }
        let pixelBuffer: CVPixelBuffer
        if key != nil, key == preparedFrameKey, let cached = preparedPixelBuffer {
            pixelBuffer = cached
        } else if frame == nil,
                  contentGeneration != 0,
                  preparedFrameKey?.contentGeneration == contentGeneration,
                  preparedFrameKey?.ringGeneration == ringGeneration,
                  preparedFrameKey?.formatIndex == formatIndex,
                  let cached = preparedPixelBuffer
        {
            // Keep emitting fresh sample timestamps without recopying an
            // unchanged source image while the ring briefly has no readable slot.
            pixelBuffer = cached
        } else {
            let allocated: CVPixelBuffer
            do {
                guard let available = try format.pool.acquire() else { return }
                allocated = available
            } catch {
                Logger.extension.error("Unable to allocate output pixel buffer: \(error)")
                return
            }
            if frame?.copyNV12(to: allocated, workspace: scaleWorkspace) != true {
                Self.fillBlack(allocated)
            }
            pixelBuffer = allocated
            preparedFrameKey = key
            preparedPixelBuffer = allocated
        }

        // A content invalidation during preparation cannot enter the system
        // sample queue. Never mutate an already-shared cached pixel buffer.
        if let preparedFrameKey,
           preparedFrameKey.contentGeneration != ringReader?.contentGeneration
        {
            self.preparedFrameKey = nil
            preparedPixelBuffer = nil
            return
        }

        guard let timestamp = Int64(exactly: tick.presentationNanoseconds) else { return }
        let presentationTime = CMTime(value: timestamp, timescale: Int32(NSEC_PER_SEC))
        var timing = CMSampleTimingInfo(
            duration: CMTime(value: 1, timescale: Int32(tick.frameRate)),
            presentationTimeStamp: presentationTime,
            decodeTimeStamp: .invalid
        )
        var sampleBuffer: CMSampleBuffer?
        guard CMSampleBufferCreateForImageBuffer(
            allocator: kCFAllocatorDefault,
            imageBuffer: pixelBuffer,
            dataReady: true,
            makeDataReadyCallback: nil,
            refcon: nil,
            formatDescription: format.description,
            sampleTiming: &timing,
            sampleBufferOut: &sampleBuffer
        ) == noErr, let sampleBuffer
        else {
            Logger.extension.error("Unable to create output sample buffer")
            return
        }

        var discontinuity: CMIOExtensionStream.DiscontinuityFlags = []
        if tick.skippedSlots > 0 || sampleWasDropped { discontinuity.insert(.sampleDropped) }
        streamSource.stream.send(
            sampleBuffer,
            discontinuity: discontinuity,
            hostTimeInNanoseconds: tick.presentationNanoseconds
        )
        sampleWasDropped = false
        emitted = true
    }

    private func scheduleNextSample() {
        guard let deadline = sampleClock.nextDeadline else { return }
        timer?.schedule(deadline: DispatchTime(uptimeNanoseconds: deadline), leeway: .milliseconds(1))
    }

    private func resetRingReader() {
        ringReader = nil
        preparedFrameKey = nil
        preparedPixelBuffer = nil
        lastRingIdentityProbeAt = 0
    }

    private static func makeOutputFormat(
        width: Int, height: Int, frameRate: UInt64, pool: OutputPixelBufferPool
    ) throws -> OutputFormat {
        var description: CMFormatDescription?
        guard CMVideoFormatDescriptionCreate(
            allocator: kCFAllocatorDefault,
            codecType: kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
            width: Int32(width),
            height: Int32(height),
            extensions: nil,
            formatDescriptionOut: &description
        ) == noErr, let description
        else {
            throw CocoaError(.coderInvalidValue)
        }

        let duration = CMTime(value: 1, timescale: Int32(frameRate))
        return OutputFormat(
            width: width,
            height: height,
            frameRate: frameRate,
            description: description,
            streamFormat: CMIOExtensionStreamFormat(
                formatDescription: description,
                maxFrameDuration: duration,
                minFrameDuration: duration,
                validFrameDurations: nil
            ),
            pool: pool
        )
    }

    private static func fillBlack(_ pixelBuffer: CVPixelBuffer) {
        guard CVPixelBufferLockBaseAddress(pixelBuffer, []) == kCVReturnSuccess else { return }
        defer { CVPixelBufferUnlockBaseAddress(pixelBuffer, []) }

        for plane in 0..<CVPixelBufferGetPlaneCount(pixelBuffer) {
            guard let base = CVPixelBufferGetBaseAddressOfPlane(pixelBuffer, plane) else { continue }
            let value: UInt8 = plane == 0 ? 16 : 128
            memset(
                base,
                Int32(value),
                CVPixelBufferGetBytesPerRowOfPlane(pixelBuffer, plane)
                    * CVPixelBufferGetHeightOfPlane(pixelBuffer, plane)
            )
        }
    }
}

final class PicooCameraStreamSource: NSObject, CMIOExtensionStreamSource, @unchecked Sendable {
    private(set) var stream: CMIOExtensionStream!
    let device: CMIOExtensionDevice
    let formats: [CMIOExtensionStreamFormat]

    private let formatLock = NSLock()
    private var selectedFormatIndex = 3

    init(
        localizedName: String,
        streamID: UUID,
        formats: [CMIOExtensionStreamFormat],
        device: CMIOExtensionDevice
    ) {
        self.device = device
        self.formats = formats
        super.init()
        stream = CMIOExtensionStream(
            localizedName: localizedName,
            streamID: streamID,
            direction: .source,
            clockType: .hostTime,
            source: self
        )
    }

    var activeFormatIndex: Int { formatLock.withLock { selectedFormatIndex } }

    var availableProperties: Set<CMIOExtensionProperty> {
        [.streamActiveFormatIndex, .streamFrameDuration]
    }

    func streamProperties(
        forProperties properties: Set<CMIOExtensionProperty>
    ) throws -> CMIOExtensionStreamProperties {
        let result = CMIOExtensionStreamProperties(dictionary: [:])
        let index = activeFormatIndex
        if properties.contains(.streamActiveFormatIndex) {
            result.activeFormatIndex = index
        }
        if properties.contains(.streamFrameDuration) {
            result.frameDuration = formats[index].minFrameDuration
        }
        return result
    }

    func setStreamProperties(_ streamProperties: CMIOExtensionStreamProperties) throws {
        try formatLock.withLock {
            var index = streamProperties.activeFormatIndex ?? selectedFormatIndex
            guard formats.indices.contains(index) else { throw CocoaError(.coderInvalidValue) }
            if let duration = streamProperties.frameDuration {
                let dimensions = CMVideoFormatDescriptionGetDimensions(formats[index].formatDescription)
                guard let matching = formats.firstIndex(where: { candidate in
                    let size = CMVideoFormatDescriptionGetDimensions(candidate.formatDescription)
                    return size.width == dimensions.width && size.height == dimensions.height
                        && CMTimeCompare(candidate.minFrameDuration, duration) == 0
                }) else { throw CocoaError(.coderInvalidValue) }
                index = matching
            }
            selectedFormatIndex = index
        }
    }

    func authorizedToStartStream(for client: CMIOExtensionClient) -> Bool { true }

    func startStream() throws {
        guard let source = device.source as? PicooCameraDeviceSource else {
            throw CocoaError(.coderInvalidValue)
        }
        source.startStreaming()
    }

    func stopStream() throws {
        guard let source = device.source as? PicooCameraDeviceSource else {
            throw CocoaError(.coderInvalidValue)
        }
        source.stopStreaming()
    }
}

final class PicooCameraProviderSource: NSObject, CMIOExtensionProviderSource, @unchecked Sendable {
    private(set) var provider: CMIOExtensionProvider!
    private let deviceSource: PicooCameraDeviceSource

    init(clientQueue: DispatchQueue) throws {
        deviceSource = try PicooCameraDeviceSource(localizedName: "Picoo Camera")
        super.init()
        provider = CMIOExtensionProvider(source: self, clientQueue: clientQueue)
        try provider.addDevice(deviceSource.device)
    }

    func connect(to client: CMIOExtensionClient) throws {}
    func disconnect(from client: CMIOExtensionClient) {}

    var availableProperties: Set<CMIOExtensionProperty> { [.providerManufacturer] }

    func providerProperties(
        forProperties properties: Set<CMIOExtensionProperty>
    ) throws -> CMIOExtensionProviderProperties {
        let result = CMIOExtensionProviderProperties(dictionary: [:])
        if properties.contains(.providerManufacturer) {
            result.manufacturer = "Picoo"
        }
        return result
    }

    func setProviderProperties(_ providerProperties: CMIOExtensionProviderProperties) throws {}
}

private extension Logger {
    static let `extension` = Logger(
        subsystem: "com.haoxincode.picoo-camera.camera-extension",
        category: "CameraExtension"
    )
}
