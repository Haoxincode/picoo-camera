import CoreMedia
import CoreMediaIO
import CoreVideo
import Foundation
import IOKit.audio
import os

struct OutputFormat {
    let width: Int
    let height: Int
    let frameRate: UInt64
    let description: CMFormatDescription
    let streamFormat: CMIOExtensionStreamFormat
    let pool: OutputPixelBufferPool
}

private struct SinkDelivery: @unchecked Sendable {
    let sampleBuffer: CMSampleBuffer
    let stream: CMIOExtensionStream
}

final class OutputFormatSelection: @unchecked Sendable {
    private let lock = NSLock()
    private var selectedFormatIndex = 3

    var activeFormatIndex: Int { lock.withLock { selectedFormatIndex } }

    func update(
        _ properties: CMIOExtensionStreamProperties,
        formats: [CMIOExtensionStreamFormat]
    ) throws -> (index: Int, changed: Bool) {
        try lock.withLock {
            var index = properties.activeFormatIndex ?? selectedFormatIndex
            guard formats.indices.contains(index) else { throw CocoaError(.coderInvalidValue) }
            if let duration = properties.frameDuration {
                let dimensions = CMVideoFormatDescriptionGetDimensions(
                    formats[index].formatDescription
                )
                guard let matching = formats.firstIndex(where: { candidate in
                    let size = CMVideoFormatDescriptionGetDimensions(candidate.formatDescription)
                    return size.width == dimensions.width && size.height == dimensions.height
                        && CMTimeCompare(candidate.minFrameDuration, duration) == 0
                }) else { throw CocoaError(.coderInvalidValue) }
                index = matching
            }
            let changed = index != selectedFormatIndex
            selectedFormatIndex = index
            return (index, changed)
        }
    }
}

final class PicooCameraDeviceSource: NSObject, CMIOExtensionDeviceSource, @unchecked Sendable {
    private(set) var device: CMIOExtensionDevice!

    private let frameQueue = DispatchQueue(
        label: "com.haoxincode.picoo-camera.extension.frames",
        qos: .userInteractive,
        autoreleaseFrequency: .workItem
    )
    private let frameQueueKey = DispatchSpecificKey<Void>()
    private var timer: DispatchSourceTimer?
    private var sampleClock = OutputSampleClock()
    private var streamingClients: UInt32 = 0
    private var sampleWasDropped = false
    private var preparedPixelBuffer: CVPixelBuffer?
    // Accessed only on frameQueue. Sink callbacks carry their generation into
    // this queue and are admitted only while it still matches.
    private var sinkGeneration: UInt64 = 0
    private let outputFormats: [OutputFormat]
    private let formatSelection = OutputFormatSelection()
    private var streamSource: PicooCameraStreamSource!
    private var sinkStreamSource: PicooCameraSinkStreamSource!

    init(localizedName: String) throws {
        outputFormats = try [(1280, 720), (1920, 1080)].flatMap { width, height in
            let pool = try OutputPixelBufferPool(width: width, height: height)
            return try [UInt64(30), UInt64(60)].map { frameRate in
                try Self.makeOutputFormat(width: width, height: height, frameRate: frameRate, pool: pool)
            }
        }
        super.init()
        frameQueue.setSpecific(key: frameQueueKey, value: ())

        guard let deviceID = UUID(uuidString: "9D0187D2-9E73-4A49-8F0D-5049434F4F01"),
              let streamID = UUID(uuidString: "9D0187D2-9E73-4A49-8F0D-5049434F4F02"),
              let sinkStreamID = UUID(uuidString: "9D0187D2-9E73-4A49-8F0D-5049434F4F03")
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
            selection: formatSelection,
            device: device
        )
        sinkStreamSource = PicooCameraSinkStreamSource(
            localizedName: "Picoo Camera Host Input",
            streamID: sinkStreamID,
            formats: outputFormats.map(\.streamFormat),
            selection: formatSelection,
            device: device
        )
        try device.addStream(streamSource.stream)
        try device.addStream(sinkStreamSource.stream)
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
            sinkStreamSource.setOutputDemand(true)
        }
    }

    func stopStreaming() {
        frameQueue.sync {
            guard streamingClients > 0 else { return }
            streamingClients -= 1
            guard streamingClients == 0 else { return }
            sinkStreamSource.setOutputDemand(false)
            timer?.cancel()
            timer = nil
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
        let pixelBuffer: CVPixelBuffer
        if let cached = preparedPixelBuffer,
           CVPixelBufferGetWidth(cached) == format.width,
           CVPixelBufferGetHeight(cached) == format.height,
           CVPixelBufferGetPixelFormatType(cached)
               == kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
        {
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
            Self.fillBlack(allocated)
            pixelBuffer = allocated
            preparedPixelBuffer = allocated
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

    func activeFormatDidChange(to index: Int) {
        frameQueue.async { [weak self] in
            self?.preparedPixelBuffer = nil
        }
        sinkStreamSource.notifyActiveFormatChanged(to: index)
    }

    func invalidateSinkSamples(generation: UInt64) {
        if DispatchQueue.getSpecific(key: frameQueueKey) != nil {
            sinkGeneration = generation
            preparedPixelBuffer = nil
            sampleWasDropped = true
        } else {
            frameQueue.sync {
                sinkGeneration = generation
                preparedPixelBuffer = nil
                sampleWasDropped = true
            }
        }
    }

    func acceptSinkSample(
        _ sampleBuffer: CMSampleBuffer,
        sequenceNumber: UInt64,
        discontinuity: CMIOExtensionStream.DiscontinuityFlags,
        generation: UInt64,
        stream: CMIOExtensionStream
    ) {
        let delivery = SinkDelivery(sampleBuffer: sampleBuffer, stream: stream)
        frameQueue.async { [weak self, delivery] in
            guard let self else { return }
            defer {
                delivery.stream.notifyScheduledOutputChanged(
                    CMIOExtensionScheduledOutput(
                        sequenceNumber: sequenceNumber,
                        hostTimeInNanoseconds: DispatchTime.now().uptimeNanoseconds
                    )
                )
            }
            guard generation == self.sinkGeneration else {
                self.sampleWasDropped = true
                return
            }
            guard let image = compatibleSinkImage(in: delivery.sampleBuffer) else {
                sampleWasDropped = true
                return
            }
            preparedPixelBuffer = image
            if !discontinuity.isEmpty { sampleWasDropped = true }
        }
    }

    func compatibleSinkImage(in sampleBuffer: CMSampleBuffer) -> CVPixelBuffer? {
        let index = formatSelection.activeFormatIndex
        guard outputFormats.indices.contains(index),
              let image = CMSampleBufferGetImageBuffer(sampleBuffer)
        else { return nil }
        let format = outputFormats[index]
        guard CVPixelBufferGetPixelFormatType(image)
                == kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
              CVPixelBufferGetWidth(image) == format.width,
              CVPixelBufferGetHeight(image) == format.height,
              CMVideoFormatDescriptionMatchesImageBuffer(
                  format.description,
                  imageBuffer: image
              )
        else { return nil }
        return image
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
            extensions: [
                kCMFormatDescriptionExtension_ColorPrimaries:
                    kCMFormatDescriptionColorPrimaries_ITU_R_709_2,
                kCMFormatDescriptionExtension_TransferFunction:
                    kCMFormatDescriptionTransferFunction_ITU_R_709_2,
                kCMFormatDescriptionExtension_YCbCrMatrix:
                    kCMFormatDescriptionYCbCrMatrix_ITU_R_709_2,
            ] as CFDictionary,
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
        applyOutputColor(to: pixelBuffer)
    }

    static func applyOutputColor(to pixelBuffer: CVPixelBuffer) {
        CVBufferSetAttachment(
            pixelBuffer,
            kCVImageBufferColorPrimariesKey,
            kCVImageBufferColorPrimaries_ITU_R_709_2,
            .shouldPropagate
        )
        CVBufferSetAttachment(
            pixelBuffer,
            kCVImageBufferTransferFunctionKey,
            kCVImageBufferTransferFunction_ITU_R_709_2,
            .shouldPropagate
        )
        CVBufferSetAttachment(
            pixelBuffer,
            kCVImageBufferYCbCrMatrixKey,
            kCVImageBufferYCbCrMatrix_ITU_R_709_2,
            .shouldPropagate
        )
    }
}

final class PicooCameraStreamSource: NSObject, CMIOExtensionStreamSource, @unchecked Sendable {
    private(set) var stream: CMIOExtensionStream!
    let device: CMIOExtensionDevice
    let formats: [CMIOExtensionStreamFormat]
    private let selection: OutputFormatSelection

    init(
        localizedName: String,
        streamID: UUID,
        formats: [CMIOExtensionStreamFormat],
        selection: OutputFormatSelection,
        device: CMIOExtensionDevice
    ) {
        self.device = device
        self.formats = formats
        self.selection = selection
        super.init()
        stream = CMIOExtensionStream(
            localizedName: localizedName,
            streamID: streamID,
            direction: .source,
            clockType: .hostTime,
            source: self
        )
    }

    var activeFormatIndex: Int { selection.activeFormatIndex }

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
        let update = try selection.update(streamProperties, formats: formats)
        if update.changed {
            guard let source = device.source as? PicooCameraDeviceSource else {
                throw CocoaError(.coderInvalidValue)
            }
            source.activeFormatDidChange(to: update.index)
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
