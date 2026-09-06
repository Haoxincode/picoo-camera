//! REQ-PICOO-GPU-007: target-only NV12 readback; never accepts a source image.
use std::sync::Arc;
use windows::core::{IUnknown, Interface};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Texture2D, D3D11_CPU_ACCESS_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_FLAG_DO_NOT_WAIT,
    D3D11_MAP_READ, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC};

use crate::{
    cpu_image::CpuImagePool, CpuImage, RenderError, RenderSpec, RenderedImage, WindowsGpuContext,
};

struct Staging(ID3D11Texture2D);
// SAFETY: The staging resource is private to one exporter. It is written only
// during an owned GPU copy and mapped only after that copy's completion event.
unsafe impl Send for Staging {}
unsafe impl Sync for Staging {}

struct ReadbackOwners {
    image: RenderedImage,
    staging: Arc<Staging>,
}

/// One lazy staging texture and three CPU outputs. No queue, timer or idle work.
/// Callers own CPU demand, source deduplication and output generation admission.
pub struct CpuExporter {
    gpu: Arc<WindowsGpuContext>,
    spec: RenderSpec,
    pool: CpuImagePool,
    staging: Option<Arc<Staging>>,
    exports: u64,
}
impl CpuExporter {
    pub fn new(gpu: Arc<WindowsGpuContext>, spec: RenderSpec) -> Result<Self, RenderError> {
        Ok(Self {
            gpu,
            spec,
            pool: CpuImagePool::new(spec)?,
            staging: None,
            exports: 0,
        })
    }
    pub fn exports(&self) -> u64 {
        self.exports
    }

    /// GPU copying and CPU mapping remain on the dedicated output worker.
    pub fn export(&mut self, image: &RenderedImage) -> Result<Arc<CpuImage>, RenderError> {
        if image.spec() != self.spec {
            return Err(RenderError::OutputLayoutMismatch);
        }
        unsafe {
            if image
                .texture()
                .GetDevice()
                .map_err(platform)?
                .cast::<IUnknown>()
                .map_err(platform)?
                != self.gpu.device.cast::<IUnknown>().map_err(platform)?
            {
                return Err(RenderError::Platform(
                    "CPU output belongs to a different D3D11 device".into(),
                ));
            }
        }
        let gpu = &self.gpu;
        let staging = &mut self.staging;
        let spec = self.spec;
        // CPU capacity is reserved before any new staging allocation or GPU copy.
        let output = self.pool.materialize(|pixels| unsafe {
            if staging.is_none() {
                *staging = Some(Arc::new(allocate_staging(gpu, spec)?));
            }
            let owners = ReadbackOwners {
                image: image.clone(),
                staging: Arc::clone(staging.as_ref().expect("allocated staging")),
            };
            let owners = gpu
                .submit_owned(owners, |gpu, owners| {
                    gpu.with_immediate_context(|context| {
                        context.CopyResource(&owners.staging.0, owners.image.texture());
                    });
                    Ok(())
                })
                .and_then(|completion| completion.wait_on_worker())
                .map_err(|error| RenderError::Platform(error.to_string()))?;
            copy_mapped(gpu, &owners.staging.0, spec, pixels)
        })?;
        self.exports = self.exports.saturating_add(1);
        Ok(output)
    }
}

fn platform(error: windows::core::Error) -> RenderError {
    RenderError::Platform(error.to_string())
}

unsafe fn allocate_staging(
    gpu: &WindowsGpuContext,
    spec: RenderSpec,
) -> Result<Staging, RenderError> {
    let mut texture = None;
    gpu.device
        .CreateTexture2D(
            &D3D11_TEXTURE2D_DESC {
                Width: spec.width,
                Height: spec.height,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_NV12,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_STAGING,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                ..Default::default()
            },
            None,
            Some(&mut texture),
        )
        .map_err(platform)?;
    Ok(Staging(texture.ok_or(RenderError::DeviceUnavailable)?))
}

struct Mapping<'a> {
    gpu: &'a WindowsGpuContext,
    texture: &'a ID3D11Texture2D,
}
impl Drop for Mapping<'_> {
    fn drop(&mut self) {
        unsafe {
            self.gpu
                .with_immediate_context(|context| context.Unmap(self.texture, 0));
        }
    }
}

unsafe fn copy_mapped(
    gpu: &WindowsGpuContext,
    texture: &ID3D11Texture2D,
    spec: RenderSpec,
    output: &mut [u8],
) -> Result<(), RenderError> {
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    gpu.with_immediate_context(|context| {
        context.Map(
            texture,
            0,
            D3D11_MAP_READ,
            D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32,
            Some(&mut mapped),
        )
    })
    .map_err(platform)?;
    let _mapping = Mapping { gpu, texture };
    let width = spec.width as usize;
    let rows = spec.height as usize * 3 / 2;
    let pitch = mapped.RowPitch as usize;
    if mapped.pData.is_null()
        || pitch < width
        || pitch
            .checked_mul(rows)
            .is_none_or(|bytes| bytes > isize::MAX as usize)
        || output.len() != width * rows
    {
        return Err(RenderError::OutputLayoutMismatch);
    }
    // DXGI NV12 staging layout is pitch*height Y followed by pitch*(height/2)
    // UV. Copy only visible rows. Do not hold the native context lock while the
    // CPU copies: other sinks must remain free to submit their GPU work.
    for row in 0..rows {
        let source = std::slice::from_raw_parts(mapped.pData.cast::<u8>().add(row * pitch), width);
        output[row * width..(row + 1) * width].copy_from_slice(source);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
