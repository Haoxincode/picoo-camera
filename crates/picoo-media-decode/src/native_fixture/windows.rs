//! Explicit diagnostic NV12 upload; never used by the production Decoder.
use crate::{windows_runtime::MfRuntimeGuard, DecodeError};
use picoo_frame_hub::{D3D11ImageLease, NativeImage};
use picoo_gpu::WindowsGpuContext;
use std::sync::Arc;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC};
use windows::Win32::Media::MediaFoundation::{
    IMFSample, MFCreateDXGISurfaceBuffer, MFCreateSample,
};

struct Upload {
    sample: Option<IMFSample>,
    texture: Option<ID3D11Texture2D>,
    runtime: Arc<dyn Send + Sync>,
}
// SAFETY: Standard MF samples are free-threaded; ownership crosses only the
// existing GPU completion callback and never exposes a mutable texture alias.
unsafe impl Send for Upload {}

pub(super) fn upload(
    width: u32,
    height: u32,
    stride: u32,
    pixels: &[u8],
) -> Result<NativeImage, DecodeError> {
    let required = u64::from(stride) * (u64::from(height) + u64::from(height) / 2);
    if width == 0
        || height == 0
        || !width.is_multiple_of(2)
        || !height.is_multiple_of(2)
        || stride < width
        || required > pixels.len() as u64
    {
        return Err(DecodeError::Platform(
            "invalid diagnostic NV12 layout".into(),
        ));
    }
    let runtime = MfRuntimeGuard::diagnostic()?;
    let gpu = Arc::new(
        WindowsGpuContext::diagnostic().map_err(|e| DecodeError::Platform(e.to_string()))?,
    );
    unsafe {
        // Reserve the same bounded completion resources used by production
        // before GPU initialization. No separate event/fence scheduler lives here.
        let uploaded = gpu
            .submit_owned(
                Upload {
                    sample: None,
                    texture: None,
                    runtime: runtime._lifetime.clone(),
                },
                |gpu, upload| {
                    gpu.device().CreateTexture2D(
                        &D3D11_TEXTURE2D_DESC {
                            Width: width,
                            Height: height,
                            MipLevels: 1,
                            ArraySize: 1,
                            Format: DXGI_FORMAT_NV12,
                            SampleDesc: DXGI_SAMPLE_DESC {
                                Count: 1,
                                Quality: 0,
                            },
                            Usage: D3D11_USAGE_DEFAULT,
                            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                            ..Default::default()
                        },
                        Some(&D3D11_SUBRESOURCE_DATA {
                            pSysMem: pixels.as_ptr().cast(),
                            SysMemPitch: stride,
                            SysMemSlicePitch: 0,
                        }),
                        Some(&mut upload.texture),
                    )?;
                    let sample = MFCreateSample()?;
                    upload.sample = Some(sample);
                    let buffer = MFCreateDXGISurfaceBuffer(
                        &ID3D11Texture2D::IID,
                        upload.texture.as_ref().unwrap(),
                        0,
                        false,
                    )?;
                    upload.sample.as_ref().unwrap().AddBuffer(&buffer)?;
                    Ok(())
                },
            )
            .and_then(|completion| completion.wait_on_worker())
            .map_err(|e| DecodeError::Platform(format!("diagnostic upload completion: {e}")))?;
        let image = D3D11ImageLease::retain_completed(
            uploaded.sample.as_ref().unwrap(),
            gpu.device(),
            uploaded.runtime,
        )
        .map_err(|error| DecodeError::Platform(error.to_string()))?;
        Ok(NativeImage::Windows(image))
    }
}
