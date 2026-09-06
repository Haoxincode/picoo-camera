import CoreVideo
import Foundation

/// REQ-PICOO-VCAM-014: retained system samples consume the same three-slot budget.
final class OutputPixelBufferPool {
    private let pool: CVPixelBufferPool
    private let allocation = [kCVPixelBufferPoolAllocationThresholdKey: 3] as CFDictionary

    init(width: Int, height: Int) throws {
        let attributes: NSDictionary = [
            kCVPixelBufferWidthKey: width,
            kCVPixelBufferHeightKey: height,
            kCVPixelBufferPixelFormatTypeKey: kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
            kCVPixelBufferIOSurfacePropertiesKey: [:] as NSDictionary,
        ]
        var created: CVPixelBufferPool?
        guard CVPixelBufferPoolCreate(kCFAllocatorDefault, nil, attributes, &created)
                == kCVReturnSuccess, let created
        else { throw CocoaError(.coderInvalidValue) }
        pool = created
    }

    /// Pool pressure is expected backpressure; never fall back to an unpooled allocation.
    func acquire() throws -> CVPixelBuffer? {
        var buffer: CVPixelBuffer?
        let status = CVPixelBufferPoolCreatePixelBufferWithAuxAttributes(
            kCFAllocatorDefault, pool, allocation, &buffer
        )
        if status == kCVReturnWouldExceedAllocationThreshold { return nil }
        guard status == kCVReturnSuccess, let buffer else {
            throw NSError(domain: "PicooCoreVideo", code: Int(status))
        }
        return buffer
    }
}
