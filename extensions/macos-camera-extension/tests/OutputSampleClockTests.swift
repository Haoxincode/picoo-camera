import CoreMedia
import CoreMediaIO
import Foundation

private func require(_ condition: @autoclosure () throws -> Bool) throws {
    guard try condition() else { throw CocoaError(.coderInvalidValue) }
}

func verifyOutputSampleClock() throws {
    for rate: UInt64 in [30, 60] {
        var clock = OutputSampleClock()
        try require(clock.tick(now: 0, frameRate: rate)?.presentationNanoseconds == 0)
        try require(clock.tick(now: 1, frameRate: rate) == nil)
        // Exact rational boundaries over one hour: no rounded-period drift.
        for slot in UInt64(1)...(rate * 3_600) {
            let due = clock.nextDeadline!
            let tick = try clock.tick(now: due, frameRate: rate)
            try require(tick?.presentationNanoseconds == slot * 1_000_000_000 / rate)
            try require(tick?.skippedSlots == 0)
        }
        try require(clock.tick(now: 0, frameRate: rate) == nil)
        let late = try clock.tick(now: 3_610_000_000_000, frameRate: rate)
        try require(late?.presentationNanoseconds == 3_610_000_000_000)
        try require(late?.skippedSlots == rate * 10 - 1)
        try require(clock.tick(now: 3_610_000_000_000, frameRate: rate) == nil)
    }
    var clock = OutputSampleClock()
    _ = try clock.tick(now: 1_000_000_000, frameRate: 30)
    try require(clock.tick(now: 1_000_000_000, frameRate: 60) == nil)
    let switched = try clock.tick(now: clock.nextDeadline!, frameRate: 60)
    try require(switched?.presentationNanoseconds == 1_016_666_667)
    try require(clock.tick(now: 1_016_666_667, frameRate: 30) == nil)
    try require(clock.tick(now: clock.nextDeadline!, frameRate: 30)?.presentationNanoseconds == 1_050_000_001)
    do {
        _ = try clock.tick(now: 2_000_000_000, frameRate: 24)
        throw CocoaError(.coderInvalidValue)
    } catch OutputSampleClock.Failure.unsupportedRate {}
    var exhausted = OutputSampleClock()
    do {
        _ = try exhausted.tick(now: UInt64.max, frameRate: 60)
        throw CocoaError(.coderInvalidValue)
    } catch OutputSampleClock.Failure.exhausted {}
    print("Output clock: 30/60 Hz rational cadence, skips, reconfiguration and exhaustion passed")
}

func verifyOutputFormatProperties() throws {
    let device = try PicooCameraDeviceSource(localizedName: "Picoo contract test")
    guard let stream = device.device.streams.first?.source as? PicooCameraStreamSource else {
        throw CocoaError(.coderInvalidValue)
    }
    try require(stream.formats.count == 4)
    try require(stream.activeFormatIndex == 3)
    let properties = CMIOExtensionStreamProperties(dictionary: [:])
    properties.activeFormatIndex = 0
    properties.frameDuration = CMTime(value: 1, timescale: 60)
    try stream.setStreamProperties(properties)
    try require(stream.activeFormatIndex == 1)
    let read = try stream.streamProperties(forProperties: [.streamFrameDuration, .streamActiveFormatIndex])
    try require(read.activeFormatIndex == 1)
    try require(read.frameDuration == CMTime(value: 1, timescale: 60))
    for index in [0, 99] {
        properties.activeFormatIndex = index
        properties.frameDuration = CMTime(value: 1, timescale: 24)
        var rejected = false
        do { try stream.setStreamProperties(properties) } catch { rejected = true }
        try require(rejected && stream.activeFormatIndex == 1)
    }
    print("CMIO output properties: four formats; atomic format/rate admission passed")
}
