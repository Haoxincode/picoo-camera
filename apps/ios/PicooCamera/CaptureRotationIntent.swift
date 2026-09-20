// REQ-PICOO-MEDIA-064: one attempt per distinct direction intent.
nonisolated struct CaptureRotationIntent {
    private(set) var requested: UInt32 = 0
    private var attempted: UInt32?

    mutating func observe(_ rotation: UInt32) {
        let rotation = rotation % 360
        if rotation != requested { attempted = nil }
        requested = rotation
    }

    mutating func take(applied: UInt32) -> UInt32? {
        guard requested != applied, attempted != requested else { return nil }
        attempted = requested
        return requested
    }

    mutating func reset() { attempted = nil }
}
