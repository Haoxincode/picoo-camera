//! Explicit Apple CPU bridge into a new IOSurface-backed target buffer.

use objc2_core_foundation::CFRetained;
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferGetBaseAddressOfPlane, CVPixelBufferGetBytesPerRowOfPlane,
    CVPixelBufferGetHeightOfPlane, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags,
    CVPixelBufferUnlockBaseAddress,
};

use super::{pool::OutputPool, CpuExporter};
use crate::{RenderError, RenderSpec, RenderedImage};

#[derive(Clone)]
pub struct CpuBridgedImage {
    buffer: CFRetained<CVPixelBuffer>,
    spec: RenderSpec,
}

// SAFETY: The buffer is published only after the write mapping closes and is
// immutable thereafter. Clones only retain the same completed allocation.
unsafe impl Send for CpuBridgedImage {}
unsafe impl Sync for CpuBridgedImage {}

impl CpuBridgedImage {
    pub fn spec(&self) -> RenderSpec {
        self.spec
    }

    /// # Safety
    /// The returned platform object is immutable output. Retain the owner until
    /// the downstream system has retained or completed every read.
    pub unsafe fn pixel_buffer(&self) -> &CVPixelBuffer {
        &self.buffer
    }
}

pub struct AppleCpuBridge {
    spec: RenderSpec,
    exporter: CpuExporter,
    pool: OutputPool,
}

impl AppleCpuBridge {
    pub fn new(spec: RenderSpec) -> Result<Self, RenderError> {
        Ok(Self {
            spec,
            exporter: CpuExporter::new(spec)?,
            pool: OutputPool::new(spec)?,
        })
    }

    /// Probe the independent IOSurface pool before doing any GPU render or
    /// CPU readback. A temporary allocation is immediately released; the
    /// worker is single-threaded, so the next export observes the same slot
    /// availability unless a retained downstream image exists.
    pub fn has_capacity(&mut self) -> bool {
        self.pool.acquire().is_ok()
    }

    pub fn export(&mut self, rendered: &RenderedImage) -> Result<CpuBridgedImage, RenderError> {
        if rendered.spec() != self.spec {
            return Err(RenderError::OutputLayoutMismatch);
        }
        let output = self.pool.acquire()?;
        let cpu = self.exporter.export(rendered)?;
        copy_tight_nv12(cpu.pixels(), self.spec, &output)?;
        // The renderer committed the validated color attachments; copy only
        // ShouldPropagate metadata onto the independent CPU-filled image.
        unsafe { rendered.pixel_buffer() }.propagate_attachments(&output);
        Ok(CpuBridgedImage {
            buffer: output,
            spec: self.spec,
        })
    }
}

struct WriteMapping<'a>(Option<&'a CVPixelBuffer>);

impl WriteMapping<'_> {
    fn finish(mut self) -> Result<(), RenderError> {
        let buffer = self.0.take().expect("live CPU bridge write mapping");
        let status =
            unsafe { CVPixelBufferUnlockBaseAddress(buffer, CVPixelBufferLockFlags::empty()) };
        if status != 0 {
            return Err(RenderError::Platform(format!(
                "CPU bridge write unlock: {status}"
            )));
        }
        Ok(())
    }
}

impl Drop for WriteMapping<'_> {
    fn drop(&mut self) {
        if let Some(buffer) = self.0.take() {
            unsafe {
                CVPixelBufferUnlockBaseAddress(buffer, CVPixelBufferLockFlags::empty());
            }
        }
    }
}

fn copy_tight_nv12(
    pixels: &[u8],
    spec: RenderSpec,
    output: &CVPixelBuffer,
) -> Result<(), RenderError> {
    let width = spec.width as usize;
    let height = spec.height as usize;
    if pixels.len() != width * height * 3 / 2
        || CVPixelBufferGetHeightOfPlane(output, 0) != height
        || CVPixelBufferGetHeightOfPlane(output, 1) != height / 2
    {
        return Err(RenderError::OutputLayoutMismatch);
    }
    let status = unsafe { CVPixelBufferLockBaseAddress(output, CVPixelBufferLockFlags::empty()) };
    if status != 0 {
        return Err(RenderError::Platform(format!(
            "CPU bridge write lock: {status}"
        )));
    }
    let mapping = WriteMapping(Some(output));
    for plane in 0..2 {
        let stride = CVPixelBufferGetBytesPerRowOfPlane(output, plane);
        let rows = if plane == 0 { height } else { height / 2 };
        let source_offset = if plane == 0 { 0 } else { width * height };
        let base = CVPixelBufferGetBaseAddressOfPlane(output, plane).cast::<u8>();
        if base.is_null() || stride < width || stride.checked_mul(rows).is_none() {
            return Err(RenderError::OutputLayoutMismatch);
        }
        for row in 0..rows {
            let source = &pixels[source_offset + row * width..source_offset + (row + 1) * width];
            let destination =
                unsafe { std::slice::from_raw_parts_mut(base.add(row * stride), width) };
            destination.copy_from_slice(source);
        }
    }
    mapping.finish()
}
