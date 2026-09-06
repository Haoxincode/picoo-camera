mod cpu_export;
mod pool;
pub use cpu_export::CpuExporter;
#[cfg(test)]
mod tests;

use objc2::{
    rc::{autoreleasepool, Retained},
    runtime::AnyObject,
    AllocAnyThread,
};
use objc2_core_foundation::{CFRetained, CGAffineTransform, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{kCGColorSpaceITUR_709, kCGColorSpaceSRGB, CGColorSpace};
use objc2_core_image::{
    kCIContextCacheIntermediates, kCIContextUseSoftwareRenderer, kCIImageColorSpace, CIColor,
    CIContext, CIImage, CIRenderDestination,
};
use objc2_core_video::*;
use objc2_foundation::{NSDictionary, NSNumber};
use objc2_metal::MTLCreateSystemDefaultDevice;
use picoo_frame_hub::NativeImage;

use crate::{OutputColor, RenderError, RenderSpec, Rotation};
use pool::OutputPool;

/// Owns a completed GPU result; consumers retain it until their GPU reads end.
/// No safe mutable native handle or CPU pixel access is exposed.
#[derive(Clone)]
pub struct RenderedImage {
    buffer: CFRetained<CVPixelBuffer>,
    spec: RenderSpec,
}

// SAFETY: Only produced after the Core Image task completes. Published buffers
// and attachments are immutable; clones retain the same pool allocation.
unsafe impl Send for RenderedImage {}
unsafe impl Sync for RenderedImage {}

impl RenderedImage {
    pub fn spec(&self) -> RenderSpec {
        self.spec
    }

    /// # Safety
    /// Do not mutate pixels or attachments. Retain a clone until downstream GPU
    /// use completes. CPU mapping belongs exclusively to an output exporter or
    /// diagnostic and must not feed the source frame bus or preview.
    pub unsafe fn pixel_buffer(&self) -> &CVPixelBuffer {
        &self.buffer
    }
}

/// Created and used on one dedicated output worker, never the UI/Receiver owner.
///
/// There is no internal task queue and at most one in-flight render. The fixed
/// output pool refuses a fourth retained buffer. Callers own bounded scheduling
/// and generation checks; changing output layout requires a new renderer.
pub struct AppleRenderer {
    context: Retained<CIContext>,
    color_space: CFRetained<CGColorSpace>,
    source_color_space: CFRetained<CGColorSpace>,
    pool: OutputPool,
    spec: RenderSpec,
}

impl AppleRenderer {
    pub fn new(spec: RenderSpec) -> Result<Self, RenderError> {
        autoreleasepool(|_| Self::create(spec))
    }

    fn create(spec: RenderSpec) -> Result<Self, RenderError> {
        spec.validate()?;
        let device = MTLCreateSystemDefaultDevice().ok_or(RenderError::DeviceUnavailable)?;
        let software = NSNumber::new_bool(false);
        // SAFETY: The option is Apple's documented Boolean; explicit Metal
        // device construction never chooses a software context as a fallback.
        let context = unsafe {
            let options = NSDictionary::from_slices(
                &[kCIContextUseSoftwareRenderer, kCIContextCacheIntermediates],
                &[
                    software.as_ref() as &AnyObject,
                    software.as_ref() as &AnyObject,
                ],
            );
            CIContext::contextWithMTLDevice_options(&device, Some(&options))
        };
        let name = unsafe {
            match spec.color {
                OutputColor::Bt601Full => kCGColorSpaceSRGB,
                OutputColor::Bt709Limited => kCGColorSpaceITUR_709,
            }
        };
        let color_space = CGColorSpace::with_name(Some(name))
            .ok_or_else(|| RenderError::Platform("output color space unavailable".into()))?;
        let source_color_space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceITUR_709 }))
            .ok_or_else(|| RenderError::Platform("source color space unavailable".into()))?;
        Ok(Self {
            context,
            color_space,
            source_color_space,
            pool: OutputPool::new(spec)?,
            spec,
        })
    }

    /// Waits for GPU completion on the caller's dedicated output worker.
    /// Input and output owners stay live through success and failure completion.
    /// A busy consumer returns PoolFull before any GPU work is submitted.
    pub fn render(&mut self, source: &NativeImage) -> Result<RenderedImage, RenderError> {
        // Dedicated Rust worker threads do not have a Cocoa event-loop pool.
        // Drain temporary graph/command objects after each completed render.
        autoreleasepool(|_| self.render_completed(source))
    }

    fn render_completed(&mut self, source: &NativeImage) -> Result<RenderedImage, RenderError> {
        let source_buffer = source.apple().ok_or(RenderError::DeviceUnavailable)?;
        // SAFETY: Read-only metadata access. Unknown interpretation must not be
        // guessed by Core Image or silently fixed by mutating a published source.
        unsafe {
            for (key, value) in [
                (
                    kCVImageBufferYCbCrMatrixKey,
                    kCVImageBufferYCbCrMatrix_ITU_R_709_2,
                ),
                (
                    kCVImageBufferColorPrimariesKey,
                    kCVImageBufferColorPrimaries_ITU_R_709_2,
                ),
                (
                    kCVImageBufferTransferFunctionKey,
                    kCVImageBufferTransferFunction_ITU_R_709_2,
                ),
            ] {
                let actual = source_buffer
                    .pixel_buffer()
                    .attachment(key, std::ptr::null_mut());
                if actual.as_deref() != Some(value.as_ref()) {
                    return Err(RenderError::UnsupportedSourceColor);
                }
            }
        }
        let output = self.pool.acquire()?;
        // SAFETY: Output is exclusively acquired and unpublished; input access
        // is read-only and the borrowed NativeImage lives through the task wait.
        unsafe {
            let matrix = match self.spec.color {
                OutputColor::Bt601Full => kCVImageBufferYCbCrMatrix_ITU_R_601_4,
                OutputColor::Bt709Limited => kCVImageBufferYCbCrMatrix_ITU_R_709_2,
            };
            let transfer = match self.spec.color {
                OutputColor::Bt601Full => kCVImageBufferTransferFunction_sRGB,
                OutputColor::Bt709Limited => kCVImageBufferTransferFunction_ITU_R_709_2,
            };
            for (key, value) in [
                (kCVImageBufferYCbCrMatrixKey, matrix),
                (
                    kCVImageBufferColorPrimariesKey,
                    kCVImageBufferColorPrimaries_ITU_R_709_2,
                ),
                (kCVImageBufferTransferFunctionKey, transfer),
            ] {
                output.set_attachment(key, value.as_ref(), CVAttachmentMode::ShouldPropagate);
            }
            // Core Image's implicit CVPixelBuffer color-space inference does
            // not guarantee the committed transfer curve. The validated source
            // contract is BT.709, supplied explicitly without changing the buffer.
            // Apple documents CGColorSpaceRef as the kCIImageColorSpace value.
            let source_space =
                &*(self.source_color_space.as_ref() as *const CGColorSpace).cast::<AnyObject>();
            let options = NSDictionary::from_slices(&[kCIImageColorSpace], &[source_space]);
            let image = CIImage::imageWithCVPixelBuffer_options(
                source_buffer.pixel_buffer(),
                Some(&options),
            );
            let image = self.transform(&image);
            let destination =
                CIRenderDestination::initWithPixelBuffer(CIRenderDestination::alloc(), &output);
            destination.setColorSpace(Some(&self.color_space));
            let task = self
                .context
                .startTaskToRender_toDestination_error(&image, &destination)
                .map_err(|error| RenderError::Platform(error.to_string()))?;
            task.waitUntilCompletedAndReturnError()
                .map_err(|error| RenderError::Platform(error.to_string()))?;
        }
        Ok(RenderedImage {
            buffer: output,
            spec: self.spec,
        })
    }

    unsafe fn transform(&self, image: &CIImage) -> Retained<CIImage> {
        let orientation = match self.spec.rotation {
            Rotation::None => 1,
            Rotation::Clockwise90 => 6,
            Rotation::Clockwise180 => 3,
            Rotation::Clockwise270 => 8,
        };
        let image = unsafe { image.imageByApplyingOrientation(orientation) };
        let extent = unsafe { image.extent() };
        let width = f64::from(self.spec.width);
        let height = f64::from(self.spec.height);
        let scale = (width / extent.size.width).min(height / extent.size.height);
        let offset_x = (width - extent.size.width * scale) / 2.0;
        let offset_y = (height - extent.size.height * scale) / 2.0;
        let mut affine = CGAffineTransform {
            a: scale,
            b: 0.0,
            c: 0.0,
            d: scale,
            tx: offset_x - extent.origin.x * scale,
            ty: offset_y - extent.origin.y * scale,
        };
        if self.spec.mirror {
            affine.a = -affine.a;
            affine.tx = width - affine.tx;
        }
        let bounds = CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: CGSize { width, height },
        };
        // SAFETY: Pure immutable image operations over finite output geometry.
        unsafe {
            let image = image.imageByApplyingTransform_highQualityDownsample(affine, true);
            let black = CIImage::imageWithColor(&CIColor::colorWithRed_green_blue_alpha(
                0.0, 0.0, 0.0, 1.0,
            ))
            .imageByCroppingToRect(bounds);
            image
                .imageByCompositingOverImage(&black)
                .imageByCroppingToRect(bounds)
        }
    }
}
