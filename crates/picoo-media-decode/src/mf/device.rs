//! REQ-PICOO-MEDIA-034: hardware admission precedes MFT media-type negotiation.
use picoo_gpu::WindowsGpuContext;
use windows::core::{IUnknown, Interface};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Texture2D, ID3D11VideoDevice, D3D11_DECODER_PROFILE_H264_VLD_NOFGT,
    D3D11_VIDEO_DECODER_DESC,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_NV12;
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_CREATE_FACTORY_FLAGS,
    DXGI_ERROR_NOT_FOUND,
};
use windows::Win32::Media::MediaFoundation::{
    IMFDXGIBuffer, IMFSample, IMFTransform, MFT_MESSAGE_SET_D3D_MANAGER, MF_SA_D3D11_AWARE,
};

use crate::DecodeError;

pub(super) enum DecoderDevice {
    Hardware(WindowsGpuContext),
    #[cfg(any(test, feature = "test-codecs"))]
    SoftwareDiagnostic,
}

impl DecoderDevice {
    pub(super) fn gpu(&self) -> Option<&WindowsGpuContext> {
        match self {
            Self::Hardware(gpu) => Some(gpu),
            #[cfg(any(test, feature = "test-codecs"))]
            Self::SoftwareDiagnostic => None,
        }
    }
}

fn platform(error: impl std::fmt::Display) -> DecodeError {
    DecodeError::Platform(format!("MF hardware device admission: {error}"))
}

pub(super) fn create_context() -> Result<WindowsGpuContext, DecodeError> {
    let factory: IDXGIFactory1 =
        unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }.map_err(platform)?;
    // Bounded initial adapter selection; never replace an active source device
    // to accommodate a sink. GPUI's native surface import must use this identity.
    for index in 0..32 {
        let adapter = match unsafe { factory.EnumAdapters1(index) } {
            Ok(adapter) => adapter,
            Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => break,
            Err(error) => return Err(platform(error)),
        };
        let description = unsafe { adapter.GetDesc1() }.map_err(platform)?;
        if description.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
            continue;
        }
        return WindowsGpuContext::for_adapter(&adapter).map_err(platform);
    }
    Err(platform("no hardware DXGI adapter"))
}

pub(super) fn attach_manager(
    transform: &IMFTransform,
    gpu: &WindowsGpuContext,
) -> Result<(), DecodeError> {
    unsafe {
        let attributes = transform.GetAttributes().map_err(platform)?;
        if attributes.GetUINT32(&MF_SA_D3D11_AWARE).map_err(platform)? != 1 {
            return Err(platform("MFT is not D3D11 aware"));
        }
        // REQ-PICOO-NEXT-009/024: bind before SetInputType/SetOutputType.
        // Never clear the manager and retry media types in software.
        transform
            .ProcessMessage(
                MFT_MESSAGE_SET_D3D_MANAGER,
                gpu.device_manager().as_raw() as usize,
            )
            .map_err(platform)?;
    }
    Ok(())
}

pub(super) unsafe fn validate_output_device(
    sample: &IMFSample,
    gpu: &WindowsGpuContext,
) -> Result<(), DecodeError> {
    if sample.GetBufferCount().map_err(platform)? != 1 {
        return Err(platform("hardware output requires one DXGI allocation"));
    }
    let buffer: IMFDXGIBuffer = sample
        .GetBufferByIndex(0)
        .map_err(platform)?
        .cast()
        .map_err(platform)?;
    let mut raw = std::ptr::null_mut();
    buffer
        .GetResource(&ID3D11Texture2D::IID, &mut raw)
        .map_err(platform)?;
    if raw.is_null() {
        return Err(platform("MFT returned no D3D11 texture"));
    }
    let texture = ID3D11Texture2D::from_raw(raw);
    let actual = texture
        .GetDevice()
        .map_err(platform)?
        .cast::<IUnknown>()
        .map_err(platform)?;
    let expected = gpu.device().cast::<IUnknown>().map_err(platform)?;
    if actual != expected {
        return Err(platform("MFT returned a different D3D11 device"));
    }
    Ok(())
}

/// Driver profile/format/size evidence complements (never replaces) MFT type admission.
pub(super) fn validate_configuration(
    gpu: &WindowsGpuContext,
    geometry: &picoo_bitstream::AvcSpsFacts,
    fps: u32,
) -> Result<(), DecodeError> {
    if !matches!(
        (geometry.visible_width, geometry.visible_height),
        (1280, 720) | (1920, 1080)
    ) || !matches!(fps, 30 | 60)
    {
        return Err(platform("unsupported source size/frame-rate combination"));
    }
    unsafe {
        let video: ID3D11VideoDevice = gpu.device().cast().map_err(platform)?;
        let profile = D3D11_DECODER_PROFILE_H264_VLD_NOFGT;
        if !video
            .CheckVideoDecoderFormat(&profile, DXGI_FORMAT_NV12)
            .map_err(platform)?
            .as_bool()
        {
            return Err(platform("adapter cannot decode H.264 to NV12"));
        }
        let description = D3D11_VIDEO_DECODER_DESC {
            Guid: profile,
            SampleWidth: geometry.coded_width,
            SampleHeight: geometry.coded_height,
            OutputFormat: DXGI_FORMAT_NV12,
        };
        if video
            .GetVideoDecoderConfigCount(&description)
            .map_err(platform)?
            == 0
        {
            return Err(platform(
                "adapter has no decoder configuration for coded geometry",
            ));
        }
    }
    Ok(())
}
