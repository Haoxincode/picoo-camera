//! Explicit output-only CPU materialization — REQ-PICOO-NEXT-029/033/034.

use std::sync::Arc;

use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferGetBaseAddressOfPlane, CVPixelBufferGetBytesPerRowOfPlane,
    CVPixelBufferGetHeight, CVPixelBufferGetHeightOfPlane, CVPixelBufferGetPlaneCount,
    CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags,
    CVPixelBufferUnlockBaseAddress,
};

use crate::{RenderError, RenderSpec, RenderedImage};

/// Immutable, tightly packed NV12 for an explicit CPU output consumer.
/// This is an output artifact, never a source frame or preview input.
pub struct CpuImage {
    spec: RenderSpec,
    pixels: Vec<u8>,
}

impl CpuImage {
    pub fn spec(&self) -> RenderSpec {
        self.spec
    }
    pub fn stride(&self) -> u32 {
        self.spec.width
    }
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}

/// A fixed-layout, three-slot CPU exporter owned by one output worker.
///
/// No timer, queue, idle readback, or source-image API exists here. The caller
/// invokes export only for explicit CPU demand and owns unique-source caching.
/// Pixels are allocated lazily. Outstanding readers exhaust the pool instead
/// of causing new allocations or waiting for consumer release.
pub struct CpuExporter {
    spec: RenderSpec,
    slots: Vec<Arc<CpuImage>>,
    exports: u64,
}

impl CpuExporter {
    pub fn new(spec: RenderSpec) -> Result<Self, RenderError> {
        spec.validate()?;
        Ok(Self {
            spec,
            slots: Vec::with_capacity(3),
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
        let mut available = None;
        for (ix, slot) in self.slots.iter_mut().enumerate() {
            // get_mut also excludes outstanding Weak handles, so a concurrent
            // Weak::upgrade cannot race the writable lease.
            if Arc::get_mut(slot).is_some() {
                available = Some(ix);
                break;
            }
        }
        let ix = match available {
            Some(ix) => ix,
            None if self.slots.len() < 3 => {
                let length = self.spec.width as usize * self.spec.height as usize * 3 / 2;
                self.slots.push(Arc::new(CpuImage {
                    spec: self.spec,
                    pixels: vec![0; length],
                }));
                self.slots.len() - 1
            }
            None => return Err(RenderError::PoolFull),
        };
        let output = Arc::get_mut(&mut self.slots[ix]).ok_or(RenderError::PoolFull)?;
        // SAFETY: Read-only access, completed GPU writes, and `image` stays live
        // for the complete synchronous mapping interval. No native alias escapes.
        let buffer = unsafe { image.pixel_buffer() };
        copy_nv12(buffer, self.spec, &mut output.pixels)?;
        self.exports = self.exports.saturating_add(1);
        Ok(Arc::clone(&self.slots[ix]))
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
