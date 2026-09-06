import CoreImage
import CoreVideo
import Metal
let device = MTLCreateSystemDefaultDevice()!
let context = CIContext(mtlDevice: device, options: [.useSoftwareRenderer: false])
let rect = CGRect(x: 0, y: 0, width: 1280, height: 720)
for matrix in [kCVImageBufferYCbCrMatrix_ITU_R_601_4, kCVImageBufferYCbCrMatrix_ITU_R_709_2] {
    var pixelBuffer: CVPixelBuffer?
    let attributes: [String: Any] = [kCVPixelBufferIOSurfacePropertiesKey as String: [:], kCVPixelBufferMetalCompatibilityKey as String: true]
    precondition(CVPixelBufferCreate(nil, 1280, 720, kCVPixelFormatType_420YpCbCr8BiPlanarFullRange, attributes as CFDictionary, &pixelBuffer) == kCVReturnSuccess)
    let output = pixelBuffer!
    CVBufferSetAttachment(output, kCVImageBufferYCbCrMatrixKey, matrix, .shouldPropagate)
    CVBufferSetAttachment(output, kCVImageBufferColorPrimariesKey, kCVImageBufferColorPrimaries_ITU_R_709_2, .shouldPropagate)
    CVBufferSetAttachment(output, kCVImageBufferTransferFunctionKey, kCVImageBufferTransferFunction_ITU_R_709_2, .shouldPropagate)
    let destination = CIRenderDestination(pixelBuffer: output)
    destination.colorSpace = CGColorSpace(name: CGColorSpace.sRGB)!
    let task = try context.startTask(toRender: CIImage(color: CIColor(red: 1, green: 0, blue: 0)).cropped(to: rect), to: destination)
    let info = try task.waitUntilCompleted()
    precondition(CVPixelBufferLockBaseAddress(output, .readOnly) == 0)
    let y = CVPixelBufferGetBaseAddressOfPlane(output, 0)!.assumingMemoryBound(to: UInt8.self)[0]
    let uv = CVPixelBufferGetBaseAddressOfPlane(output, 1)!.assumingMemoryBound(to: UInt8.self)
    let expected = matrix == kCVImageBufferYCbCrMatrix_ITU_R_601_4 ? [76, 85, 255] : [54, 99, 255]
    for (actual, reference) in zip([Int(y), Int(uv[0]), Int(uv[1])], expected) {
        precondition(abs(actual - reference) <= 1, "Unexpected target color conversion")
    }
    print("device=\(device.name) matrix=\(matrix) YUV=\(y),\(uv[0]),\(uv[1]) passes=\(info.passCount)")
    CVPixelBufferUnlockBaseAddress(output, .readOnly)
}
