import Accelerate
import CoreVideo
import Darwin
import Foundation

enum SharedRingReaderError: Error {
    case appGroupUnavailable
    case openFailed(Int32)
    case statFailed(Int32)
    case mapFailed(Int32)
    case invalidLayout
}

final class SharedRingReader: @unchecked Sendable {
    static let appGroupInfoKey = "PicooAppGroupIdentifier"
    static let ringFileName = "picoo-camera-v1.ring"

    fileprivate let base: UnsafeMutableRawPointer
    fileprivate let descriptor: Int32
    private let mappedLength: Int
    private let fileURL: URL
    private let mappedDevice: dev_t
    private let mappedInode: ino_t

    static func openAppGroupRing() throws -> SharedRingReader {
        guard let appGroupIdentifier = Bundle.main.object(
            forInfoDictionaryKey: appGroupInfoKey
        ) as? String, !appGroupIdentifier.isEmpty else {
            throw SharedRingReaderError.appGroupUnavailable
        }
        guard let container = FileManager.default.containerURL(
            forSecurityApplicationGroupIdentifier: appGroupIdentifier
        ) else {
            throw SharedRingReaderError.appGroupUnavailable
        }
        return try SharedRingReader(fileURL: container.appendingPathComponent(ringFileName))
    }

    init(fileURL: URL) throws {
        let descriptor = Darwin.open(fileURL.path, O_RDWR | O_CLOEXEC)
        guard descriptor >= 0 else {
            throw SharedRingReaderError.openFailed(errno)
        }
        var status = stat()
        guard fstat(descriptor, &status) == 0 else {
            Darwin.close(descriptor)
            throw SharedRingReaderError.statFailed(errno)
        }
        let length = Int(status.st_size)
        let mapping = mmap(nil, length, PROT_READ | PROT_WRITE, MAP_SHARED, descriptor, 0)
        guard mapping != MAP_FAILED, let mapping else {
            Darwin.close(descriptor)
            throw SharedRingReaderError.mapFailed(errno)
        }
        guard picoo_ring_validate_layout(mapping, length) else {
            munmap(mapping, length)
            Darwin.close(descriptor)
            throw SharedRingReaderError.invalidLayout
        }
        base = mapping
        self.descriptor = descriptor
        mappedLength = length
        self.fileURL = fileURL
        mappedDevice = status.st_dev
        mappedInode = status.st_ino
    }

    deinit {
        munmap(base, mappedLength)
        Darwin.close(descriptor)
    }

    func acquireLatestFrame() -> SharedRingFrame? {
        var lease = PicooRingFrameLease()
        let acquired = fileURL.path.withCString {
            picoo_ring_acquire_latest($0, base, mappedLength, &lease)
        }
        guard acquired else {
            return nil
        }
        return SharedRingFrame(reader: self, lease: lease)
    }

    func stillMapsCurrentFile() -> Bool {
        let currentDescriptor = Darwin.open(fileURL.path, O_RDONLY | O_CLOEXEC)
        guard currentDescriptor >= 0 else { return false }
        defer { Darwin.close(currentDescriptor) }
        var current = stat()
        guard fstat(currentDescriptor, &current) == 0 else {
            return false
        }
        return current.st_dev == mappedDevice && current.st_ino == mappedInode
    }
}

final class VImageScaleWorkspace {
    private var storage: [UInt8] = []

    func withBuffer<T>(minimumByteCount: Int, _ body: (UnsafeMutableRawPointer?) -> T) -> T {
        guard minimumByteCount > 0 else { return body(nil) }
        if storage.count < minimumByteCount {
            storage = [UInt8](repeating: 0, count: minimumByteCount)
        }
        return storage.withUnsafeMutableBytes { body($0.baseAddress) }
    }
}

final class SharedRingFrame: @unchecked Sendable {
    let sequence: UInt64
    let timestampMicroseconds: UInt64
    let width: Int
    let height: Int
    let stride: Int
    let rotation: UInt32

    private let reader: SharedRingReader
    private var lease: PicooRingFrameLease

    fileprivate init(reader: SharedRingReader, lease: PicooRingFrameLease) {
        self.reader = reader
        self.lease = lease
        sequence = lease.sequence
        timestampMicroseconds = lease.timestamp_us
        width = Int(lease.width)
        height = Int(lease.height)
        stride = Int(lease.stride)
        rotation = lease.rotation
    }

    deinit {
        picoo_ring_release(reader.base, &lease)
    }

    func copyNV12(to pixelBuffer: CVPixelBuffer, workspace: VImageScaleWorkspace) -> Bool {
        let (yBytes, yOverflow) = stride.multipliedReportingOverflow(by: height)
        let (uvBytes, uvOverflow) = stride.multipliedReportingOverflow(by: height / 2)
        let (requiredBytes, totalOverflow) = yBytes.addingReportingOverflow(uvBytes)
        guard width > 0,
              height > 0,
              width.isMultiple(of: 2),
              height.isMultiple(of: 2),
              stride >= width,
              !yOverflow,
              !uvOverflow,
              !totalOverflow,
              CVPixelBufferGetPixelFormatType(pixelBuffer)
                == kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
              CVPixelBufferGetPlaneCount(pixelBuffer) == 2,
              Int(lease.data_length) >= requiredBytes
        else {
            return false
        }

        guard CVPixelBufferLockBaseAddress(pixelBuffer, []) == kCVReturnSuccess else {
            return false
        }
        defer { CVPixelBufferUnlockBaseAddress(pixelBuffer, []) }

        guard let yDestination = CVPixelBufferGetBaseAddressOfPlane(pixelBuffer, 0),
              let uvDestination = CVPixelBufferGetBaseAddressOfPlane(pixelBuffer, 1)
        else {
            return false
        }
        let yDestinationStride = CVPixelBufferGetBytesPerRowOfPlane(pixelBuffer, 0)
        let uvDestinationStride = CVPixelBufferGetBytesPerRowOfPlane(pixelBuffer, 1)
        let targetWidth = CVPixelBufferGetWidth(pixelBuffer)
        let targetHeight = CVPixelBufferGetHeight(pixelBuffer)
        guard targetWidth > 0,
              targetHeight > 0,
              targetWidth.isMultiple(of: 2),
              targetHeight.isMultiple(of: 2),
              yDestinationStride >= targetWidth,
              uvDestinationStride >= targetWidth
        else {
            return false
        }

        let source = reader.base.advanced(by: Int(lease.pixel_offset))
        let sourceUV = source.advanced(by: stride * height)
        if targetWidth == width, targetHeight == height {
            for row in 0..<height {
                memcpy(
                    yDestination.advanced(by: row * yDestinationStride),
                    source.advanced(by: row * stride),
                    width
                )
            }
            for row in 0..<(height / 2) {
                memcpy(
                    uvDestination.advanced(by: row * uvDestinationStride),
                    sourceUV.advanced(by: row * stride),
                    width
                )
            }
            return true
        }

        memset(yDestination, 16, yDestinationStride * targetHeight)
        memset(uvDestination, 128, uvDestinationStride * (targetHeight / 2))
        let scale = min(
            Double(targetWidth) / Double(width),
            Double(targetHeight) / Double(height)
        )
        let fittedWidth = max(2, Int(Double(width) * scale) & ~1)
        let fittedHeight = max(2, Int(Double(height) * scale) & ~1)
        let x = ((targetWidth - fittedWidth) / 2) & ~1
        let y = ((targetHeight - fittedHeight) / 2) & ~1

        var sourceY = vImage_Buffer(
            data: source,
            height: vImagePixelCount(height),
            width: vImagePixelCount(width),
            rowBytes: stride
        )
        var destinationY = vImage_Buffer(
            data: yDestination.advanced(by: y * yDestinationStride + x),
            height: vImagePixelCount(fittedHeight),
            width: vImagePixelCount(fittedWidth),
            rowBytes: yDestinationStride
        )
        var sourceCbCr = vImage_Buffer(
            data: sourceUV,
            height: vImagePixelCount(height / 2),
            width: vImagePixelCount(width / 2),
            rowBytes: stride
        )
        var destinationCbCr = vImage_Buffer(
            data: uvDestination.advanced(by: (y / 2) * uvDestinationStride + x),
            height: vImagePixelCount(fittedHeight / 2),
            width: vImagePixelCount(fittedWidth / 2),
            rowBytes: uvDestinationStride
        )
        let flags = vImage_Flags(kvImageHighQualityResampling)
        let queryFlags = flags | vImage_Flags(kvImageGetTempBufferSize)
        let yWorkspaceSize = vImageScale_Planar8(
            &sourceY,
            &destinationY,
            nil,
            queryFlags
        )
        let uvWorkspaceSize = vImageScale_CbCr8(
            &sourceCbCr,
            &destinationCbCr,
            nil,
            queryFlags
        )
        guard yWorkspaceSize >= 0, uvWorkspaceSize >= 0 else { return false }
        let workspaceSize = max(Int(yWorkspaceSize), Int(uvWorkspaceSize))
        return workspace.withBuffer(minimumByteCount: workspaceSize) { temporary in
            vImageScale_Planar8(&sourceY, &destinationY, temporary, flags) == kvImageNoError
                && vImageScale_CbCr8(
                    &sourceCbCr,
                    &destinationCbCr,
                    temporary,
                    flags
                ) == kvImageNoError
        }
    }
}
