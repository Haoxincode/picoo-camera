import CoreMedia
import CoreMediaIO
import Foundation
import os

private let picooHostSigningID = "com.haoxincode.picoo-camera"

private struct SinkClientReference: @unchecked Sendable {
    let client: CMIOExtensionClient
}

final class PicooCameraSinkStreamSource: NSObject, CMIOExtensionStreamSource, @unchecked Sendable {
    private(set) var stream: CMIOExtensionStream!
    let device: CMIOExtensionDevice
    let formats: [CMIOExtensionStreamFormat]
    private let selection: OutputFormatSelection
    private let lock = NSLock()
    private var consumptionGeneration: UInt64 = 0
    private var pendingClients = Set<UUID>()
    private var sinkStarted = false
    private var outputDemand = false

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
            direction: .sink,
            clockType: .hostTime,
            source: self
        )
    }

    var availableProperties: Set<CMIOExtensionProperty> {
        [
            .streamActiveFormatIndex,
            .streamFrameDuration,
            .streamSinkBufferQueueSize,
            .streamSinkBuffersRequiredForStartup,
        ]
    }

    func streamProperties(
        forProperties properties: Set<CMIOExtensionProperty>
    ) throws -> CMIOExtensionStreamProperties {
        let result = CMIOExtensionStreamProperties(dictionary: [:])
        let index = selection.activeFormatIndex
        guard formats.indices.contains(index) else { throw CocoaError(.coderInvalidValue) }
        if properties.contains(.streamActiveFormatIndex) {
            result.activeFormatIndex = index
        }
        if properties.contains(.streamFrameDuration) {
            result.frameDuration = formats[index].minFrameDuration
        }
        if properties.contains(.streamSinkBufferQueueSize) {
            result.sinkBufferQueueSize = 3
        }
        if properties.contains(.streamSinkBuffersRequiredForStartup) {
            result.sinkBuffersRequiredForStartup = 1
        }
        return result
    }

    func setStreamProperties(_ streamProperties: CMIOExtensionStreamProperties) throws {
        let requestedIndex = streamProperties.activeFormatIndex
        let requestedDuration = streamProperties.frameDuration
        let index = selection.activeFormatIndex
        guard requestedIndex.map({ $0 == index }) ?? true,
              requestedDuration.map({
                  CMTimeCompare($0, formats[index].minFrameDuration) == 0
              }) ?? true,
              streamProperties.sinkBufferQueueSize.map({ $0 == 3 }) ?? true,
              streamProperties.sinkBuffersRequiredForStartup.map({ $0 == 1 }) ?? true
        else {
            throw CocoaError(.featureUnsupported)
        }
    }

    func authorizedToStartStream(for client: CMIOExtensionClient) -> Bool {
        client.signingID == picooHostSigningID
    }

    func startStream() throws {
        let start = lock.withLock { () -> (UInt64, Bool) in
            consumptionGeneration &+= 1
            sinkStarted = true
            return (consumptionGeneration, outputDemand)
        }
        (device.source as? PicooCameraDeviceSource)?.invalidateSinkSamples(
            generation: start.0
        )
        guard start.1 else { return }
        for client in stream.streamingClients where authorizedToStartStream(for: client) {
            consumeNext(from: client, generation: start.0)
        }
    }

    func stopStream() throws {
        let generation = lock.withLock { () -> UInt64 in
            consumptionGeneration &+= 1
            sinkStarted = false
            return consumptionGeneration
        }
        (device.source as? PicooCameraDeviceSource)?.invalidateSinkSamples(
            generation: generation
        )
    }

    func setOutputDemand(_ demanded: Bool) {
        let transition = lock.withLock { () -> (UInt64, Bool) in
            guard outputDemand != demanded else {
                return (consumptionGeneration, false)
            }
            outputDemand = demanded
            consumptionGeneration &+= 1
            return (consumptionGeneration, demanded && sinkStarted)
        }
        (device.source as? PicooCameraDeviceSource)?.invalidateSinkSamples(
            generation: transition.0
        )
        guard transition.1 else { return }
        for client in stream.streamingClients where authorizedToStartStream(for: client) {
            consumeNext(from: client, generation: transition.0)
        }
    }

    func notifyActiveFormatChanged(to index: Int) {
        guard !stream.streamingClients.isEmpty else { return }
        let state = CMIOExtensionPropertyState<AnyObject>(value: NSNumber(value: index))
        stream.notifyPropertiesChanged([.streamActiveFormatIndex: state])
    }

    private func consumeNext(from client: CMIOExtensionClient, generation: UInt64) {
        let clientID = client.clientID
        let admitted = lock.withLock { () -> Bool in
            guard generation == consumptionGeneration,
                  outputDemand,
                  sinkStarted,
                  pendingClients.insert(clientID).inserted
            else { return false }
            return true
        }
        guard admitted else { return }
        let clientReference = SinkClientReference(client: client)
        stream.consumeSampleBuffer(from: client) { [weak self, clientReference] sample, sequence, flags, _, error in
            guard let self else { return }
            let disposition = self.lock.withLock { () -> (current: Bool, restart: UInt64?) in
                self.pendingClients.remove(clientID)
                let current = generation == self.consumptionGeneration
                let restart = !current && self.outputDemand && self.sinkStarted
                    ? self.consumptionGeneration : nil
                return (current, restart)
            }
            guard disposition.current else {
                if sample != nil {
                    self.notifyConsumed(sequenceNumber: sequence)
                }
                if let restart = disposition.restart {
                    self.consumeNext(from: clientReference.client, generation: restart)
                }
                return
            }
            if let sample {
                (self.device.source as? PicooCameraDeviceSource)?.acceptSinkSample(
                    sample,
                    sequenceNumber: sequence,
                    discontinuity: flags,
                    generation: generation,
                    stream: self.stream
                )
            } else {
                if let error {
                    Logger.picooSink.error("CMIO sink consume failed: \(error)")
                } else {
                    Logger.picooSink.error("CMIO sink consume returned no sample")
                }
                // A failed consume must not recurse synchronously: a platform
                // error can otherwise become an unbounded hot loop. Retry
                // through the generation gate after a bounded backoff.
                DispatchQueue.global(qos: .userInteractive).asyncAfter(
                    deadline: .now() + .milliseconds(50)
                ) { [weak self, clientReference] in
                    self?.consumeNext(from: clientReference.client, generation: generation)
                }
                return
            }
            self.consumeNext(from: clientReference.client, generation: generation)
        }
    }

    private func notifyConsumed(sequenceNumber: UInt64) {
        stream.notifyScheduledOutputChanged(
            CMIOExtensionScheduledOutput(
                sequenceNumber: sequenceNumber,
                hostTimeInNanoseconds: DispatchTime.now().uptimeNanoseconds
            )
        )
    }
}

private extension Logger {
    static let picooSink = Logger(
        subsystem: "com.haoxincode.picoo-camera.camera-extension",
        category: "CameraSink"
    )
}
