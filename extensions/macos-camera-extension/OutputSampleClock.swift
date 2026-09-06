/// REQ-PICOO-VCAM-015: a source-independent, drift-free 30/60 Hz host timeline.
struct OutputSampleClock {
    enum Failure: Error { case unsupportedRate, exhausted }
    struct Tick {
        let presentationNanoseconds: UInt64
        let frameRate: UInt64
        let skippedSlots: UInt64
    }

    private var rate: UInt64 = 0
    private var anchor: UInt64?
    private var lastSlot: UInt64?
    private var lastPresentation: UInt64?
    private(set) var nextDeadline: UInt64?

    mutating func tick(now: UInt64, frameRate: UInt64) throws -> Tick? {
        guard frameRate == 30 || frameRate == 60 else { throw Failure.unsupportedRate }
        if rate != frameRate {
            rate = frameRate
            anchor = try lastPresentation.map {
                try Self.add($0, Self.offset(slot: 1, rate: frameRate, ceil: true))
            } ?? now
            lastSlot = nil
            nextDeadline = anchor
        }
        guard let anchor, now >= anchor else { return nil }
        let elapsed = now - anchor
        let slot = try Self.add(
            Self.multiply(elapsed / 1_000_000_000, rate),
            (elapsed % 1_000_000_000) * rate / 1_000_000_000
        )
        if let lastSlot, slot <= lastSlot { return nil }
        let presentation = try Self.add(anchor, Self.offset(slot: slot, rate: rate, ceil: false))
        let next = try Self.add(anchor, Self.offset(slot: Self.add(slot, 1), rate: rate, ceil: true))
        let skipped = lastSlot.map { slot - $0 - 1 } ?? slot
        lastSlot = slot
        lastPresentation = presentation
        nextDeadline = next
        return Tick(presentationNanoseconds: presentation, frameRate: rate, skippedSlots: skipped)
    }

    private static func offset(slot: UInt64, rate: UInt64, ceil: Bool) throws -> UInt64 {
        let remainder = (slot % rate) * 1_000_000_000
        return try add(multiply(slot / rate, 1_000_000_000),
                       (remainder + (ceil ? rate - 1 : 0)) / rate)
    }

    private static func add(_ lhs: UInt64, _ rhs: UInt64) throws -> UInt64 {
        let (value, overflow) = lhs.addingReportingOverflow(rhs)
        guard !overflow else { throw Failure.exhausted }
        return value
    }

    private static func multiply(_ lhs: UInt64, _ rhs: UInt64) throws -> UInt64 {
        let (value, overflow) = lhs.multipliedReportingOverflow(by: rhs)
        guard !overflow else { throw Failure.exhausted }
        return value
    }
}
