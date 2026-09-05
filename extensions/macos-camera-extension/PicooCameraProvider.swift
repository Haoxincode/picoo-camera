import CoreMedia
import CoreMediaIO
import CoreVideo
import Foundation
import IOKit.audio
import os

private let frameRate: Int32 = 30
private let ringIdentityProbeIntervalNanoseconds: UInt64 = NSEC_PER_SEC

private struct PreparedFrameKey: Equatable {
    let ringGeneration: UInt64
    let sequence: UInt64
    let formatIndex: Int
}

private struct OutputFormat {
    let width: Int
    let height: Int
    let description: CMFormatDescription
    let streamFormat: CMIOExtensionStreamFormat
    let pool: CVPixelBufferPool
}

final class PicooCameraDeviceSource: NSObject, CMIOExtensionDeviceSource, @unchecked Sendable {
    private(set) var device: CMIOExtensionDevice!

    private let frameQueue = DispatchQueue(
        label: "com.haoxincode.picoo-camera.extension.frames",
        qos: .userInteractive,
        autoreleaseFrequency: .workItem
    )
    private var timer: DispatchSourceTimer?
    private var streamingClients: UInt32 = 0
    private var ringReader: SharedRingReader?
    private var ringGeneration: UInt64 = 0
    private var lastRingIdentityProbeAt: UInt64 = 0
    private var lastSequence: UInt64 = 0
    private var preparedFrameKey: PreparedFrameKey?
    private var preparedPixelBuffer: CVPixelBuffer?
    private let outputFormats: [OutputFormat]
    private var streamSource: PicooCameraStreamSource!

    init(localizedName: String) throws {
        outputFormats = try [(854, 480), (1280, 720), (1920, 1080)].map {
            try Self.makeOutputFormat(width: $0.0, height: $0.1)
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
            timer.schedule(
                deadline: .now(),
                repeating: .nanoseconds(Int(NSEC_PER_SEC) / Int(frameRate)),
                leeway: .milliseconds(1)
            )
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
            timer?.cancel()
            timer = nil
            ringReader = nil
            preparedFrameKey = nil
            preparedPixelBuffer = nil
            lastSequence = 0
        }
    }

    private func emitFrame() {
        let formatIndex = streamSource.activeFormatIndex
        guard outputFormats.indices.contains(formatIndex) else { return }
        let format = outputFormats[formatIndex]

        let now = DispatchTime.now().uptimeNanoseconds
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
        let frame = ringReader?.acquireLatestFrame()
        if frame == nil, ringReader?.stillMapsCurrentFile() == false {
            resetRingReader()
        }

        let key = frame.map {
            PreparedFrameKey(
                ringGeneration: ringGeneration,
                sequence: $0.sequence,
                formatIndex: formatIndex
            )
        }
        let pixelBuffer: CVPixelBuffer
        if key != nil, key == preparedFrameKey, let cached = preparedPixelBuffer {
            pixelBuffer = cached
        } else if frame == nil,
                  preparedFrameKey?.ringGeneration == ringGeneration,
                  preparedFrameKey?.formatIndex == formatIndex,
                  let cached = preparedPixelBuffer
        {
            // Keep emitting fresh sample timestamps without recopying an
            // unchanged source image while the ring briefly has no readable slot.
            pixelBuffer = cached
        } else {
            var allocated: CVPixelBuffer?
            guard CVPixelBufferPoolCreatePixelBuffer(
                kCFAllocatorDefault,
                format.pool,
                &allocated
            ) == kCVReturnSuccess, let allocated
            else {
                Logger.extension.error("Unable to allocate output pixel buffer")
                return
            }
            if frame?.copyNV12(to: allocated) != true {
                Self.fillBlack(allocated)
            }
            pixelBuffer = allocated
            preparedFrameKey = key
            preparedPixelBuffer = allocated
        }

        let presentationTime = CMClockGetTime(CMClockGetHostTimeClock())
        var timing = CMSampleTimingInfo(
            duration: CMTime(value: 1, timescale: frameRate),
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
        if let frame {
            if lastSequence != 0, frame.sequence > lastSequence + 1 {
                discontinuity.insert(.sampleDropped)
            }
            lastSequence = frame.sequence
        }
        streamSource.stream.send(
            sampleBuffer,
            discontinuity: discontinuity,
            // CMIO expects nanoseconds, while
            // CMClockConvertHostTimeToSystemUnits returns mach_absolute_time units.
            hostTimeInNanoseconds: UInt64(
                presentationTime.seconds * Double(NSEC_PER_SEC)
            )
        )
    }

    private func resetRingReader() {
        ringReader = nil
        preparedFrameKey = nil
        preparedPixelBuffer = nil
        lastSequence = 0
        lastRingIdentityProbeAt = 0
    }

    private static func makeOutputFormat(width: Int, height: Int) throws -> OutputFormat {
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

        let attributes: NSDictionary = [
            kCVPixelBufferWidthKey: width,
            kCVPixelBufferHeightKey: height,
            kCVPixelBufferPixelFormatTypeKey:
                kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
            kCVPixelBufferIOSurfacePropertiesKey: [:] as NSDictionary,
        ]
        var pool: CVPixelBufferPool?
        guard CVPixelBufferPoolCreate(
            kCFAllocatorDefault,
            nil,
            attributes,
            &pool
        ) == kCVReturnSuccess, let pool
        else {
            throw CocoaError(.coderInvalidValue)
        }

        let duration = CMTime(value: 1, timescale: frameRate)
        return OutputFormat(
            width: width,
            height: height,
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
    private var selectedFormatIndex = 2

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

    var activeFormatIndex: Int {
        get { formatLock.withLock { selectedFormatIndex } }
        set {
            formatLock.withLock {
                guard formats.indices.contains(newValue) else { return }
                selectedFormatIndex = newValue
            }
        }
    }

    var availableProperties: Set<CMIOExtensionProperty> {
        [.streamActiveFormatIndex, .streamFrameDuration]
    }

    func streamProperties(
        forProperties properties: Set<CMIOExtensionProperty>
    ) throws -> CMIOExtensionStreamProperties {
        let result = CMIOExtensionStreamProperties(dictionary: [:])
        if properties.contains(.streamActiveFormatIndex) {
            result.activeFormatIndex = activeFormatIndex
        }
        if properties.contains(.streamFrameDuration) {
            result.frameDuration = CMTime(value: 1, timescale: frameRate)
        }
        return result
    }

    func setStreamProperties(_ streamProperties: CMIOExtensionStreamProperties) throws {
        if let index = streamProperties.activeFormatIndex {
            guard formats.indices.contains(index) else {
                throw CocoaError(.coderInvalidValue)
            }
            activeFormatIndex = index
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
