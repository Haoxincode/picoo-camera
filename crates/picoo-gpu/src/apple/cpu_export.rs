//! Explicit output-only CPU materialization — REQ-PICOO-NEXT-029/033/034.

use std::sync::Arc;

use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferGetBaseAddressOfPlane, CVPixelBufferGetBytesPerRowOfPlane,
    CVPixelBufferGetHeight, CVPixelBufferGetHeightOfPlane, CVPixelBufferGetPlaneCount,
    CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags,
    CVPixelBufferUnlockBaseAddress,
};

use crate::{cpu_image::CpuImagePool, CpuImage, RenderError, RenderSpec, RenderedImage};

/// A fixed-layout, three-slot CPU exporter owned by one output worker.
///
/// No timer, queue, idle readback, or source-image API exists here. The caller
/// invokes export only for explicit CPU demand and owns unique-source caching.
/// Pixels are allocated lazily. Outstanding readers exhaust the pool instead
/// of causing new allocations or waiting for consumer release.
pub struct CpuExporter {
    spec: RenderSpec,
    pool: CpuImagePool,
    exports: u64,
}

impl CpuExporter {
    pub fn new(spec: RenderSpec) -> Result<Self, RenderError> {
        Ok(Self {
            spec,
            pool: CpuImagePool::new(spec)?,
            exports: 0,
        })
    }

    pub fn exports(&self) -> u64 {
        self.exports
    }

    /// Performs the sole CPU mapping/copy on the dedicated output worker.
    /// The input's GPU write has already completed before RenderedImage exists.
    pub fn export(&mut self, image: &RenderedImage) -> Result<Arc<CpuImage>, RenderError> {
        if image.spec() != self.spec {
            return Err(RenderError::OutputLayoutMismatch);
        }
        let output = self.pool.materialize(|pixels| {
            // SAFETY: The completed target remains retained throughout mapping.
            let buffer = unsafe { image.pixel_buffer() };
            copy_nv12(buffer, self.spec, pixels)
        })?;
        self.exports = self.exports.saturating_add(1);
        Ok(output)
    }
}

struct ReadMapping<'a>(Option<&'a CVPixelBuffer>);

impl ReadMapping<'_> {
    fn finish(mut self) -> Result<(), RenderError> {
        let buffer = self.0.take().expect("live CPU read mapping");
        // SAFETY: The matching successful read lock is still active.
        let status =
            unsafe { CVPixelBufferUnlockBaseAddress(buffer, CVPixelBufferLockFlags::ReadOnly) };
        if status != 0 {
            return Err(RenderError::Platform(format!(
                "CPU output read unlock: {status}"
            )));
        }
        Ok(())
    }
}

impl Drop for ReadMapping<'_> {
    fn drop(&mut self) {
        // SAFETY: Created only after a successful read lock; exactly one unlock.
        if let Some(buffer) = self.0.take() {
            unsafe {
                CVPixelBufferUnlockBaseAddress(buffer, CVPixelBufferLockFlags::ReadOnly);
            }
        }
    }
}

fn copy_nv12(
    buffer: &CVPixelBuffer,
    spec: RenderSpec,
    output: &mut [u8],
) -> Result<(), RenderError> {
    let width = spec.width as usize;
    let height = spec.height as usize;
    if CVPixelBufferGetWidth(buffer) != width
        || CVPixelBufferGetHeight(buffer) != height
        || CVPixelBufferGetPlaneCount(buffer) != 2
        || CVPixelBufferGetHeightOfPlane(buffer, 0) != height
        || CVPixelBufferGetHeightOfPlane(buffer, 1) != height / 2
    {
        return Err(RenderError::OutputLayoutMismatch);
    }
    // SAFETY: Valid retained pixel buffer; access remains read-only.
    let status = unsafe { CVPixelBufferLockBaseAddress(buffer, CVPixelBufferLockFlags::ReadOnly) };
    if status != 0 {
        return Err(RenderError::Platform(format!(
            "CPU output read lock: {status}"
        )));
    }
    let mapping = ReadMapping(Some(buffer));
    for plane in 0..2 {
        let stride = CVPixelBufferGetBytesPerRowOfPlane(buffer, plane);
        let rows = if plane == 0 { height } else { height / 2 };
        let offset = if plane == 0 { 0 } else { width * height };
        // Address queries occur only while the read mapping is held.
        let base = CVPixelBufferGetBaseAddressOfPlane(buffer, plane).cast::<u8>();
        if base.is_null() || stride < width || stride.checked_mul(rows).is_none() {
            return Err(RenderError::OutputLayoutMismatch);
        }
        for row in 0..rows {
            // SAFETY: Platform plane metadata guarantees `stride` bytes per
            // reported row; only visible width is copied, never padding.
            let source = unsafe { std::slice::from_raw_parts(base.add(row * stride), width) };
            let start = offset + row * width;
            output[start..start + width].copy_from_slice(source);
        }
    }
    mapping.finish()
}
