//! REQ-PICOO-MEDIA-034: hardware admission precedes MFT media-type negotiation.
use picoo_gpu::WindowsGpuContext;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, ID3D11VideoDevice, D3D11_DECODER_PROFILE_H264_VLD_NOFGT, D3D11_VIDEO_DECODER_DESC,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_NV12;
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_CREATE_FACTORY_FLAGS,
    DXGI_ERROR_NOT_FOUND,
};
use windows::Win32::Media::MediaFoundation::{
    IMFDXGIDeviceManager, IMFTransform, MFCreateDXGIDeviceManager, MFT_MESSAGE_SET_D3D_MANAGER,
    MF_SA_D3D11_AWARE,
};

use crate::DecodeError;

pub(super) enum DecoderDevice {
    Hardware {
        _manager: IMFDXGIDeviceManager,
        gpu: std::sync::Arc<WindowsGpuContext>,
    },
    #[cfg(any(test, feature = "test-codecs"))]
    SoftwareDiagnostic,
}

impl DecoderDevice {
    pub(super) fn gpu(&self) -> Option<&std::sync::Arc<WindowsGpuContext>> {
        match self {
            Self::Hardware { gpu, .. } => Some(gpu),
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
    gpu: std::sync::Arc<WindowsGpuContext>,
) -> Result<DecoderDevice, DecodeError> {
    unsafe {
        let attributes = transform.GetAttributes().map_err(platform)?;
        if attributes.GetUINT32(&MF_SA_D3D11_AWARE).map_err(platform)? != 1 {
            return Err(platform("MFT is not D3D11 aware"));
        }
        let manager = create_manager(gpu.device())?;
        // REQ-PICOO-NEXT-009/024: bind before SetInputType/SetOutputType.
        // Never clear the manager and retry media types in software.
        transform
            .ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, manager.as_raw() as usize)
            .map_err(platform)?;
        Ok(DecoderDevice::Hardware {
            _manager: manager,
            gpu,
        })
    }
}

fn create_manager(device: &ID3D11Device) -> Result<IMFDXGIDeviceManager, DecodeError> {
    let mut token = 0;
    let mut manager = None;
    unsafe {
        MFCreateDXGIDeviceManager(&mut token, &mut manager).map_err(platform)?;
    }
    let manager = manager.ok_or_else(|| platform("missing DXGI device manager"))?;
    unsafe {
        manager.ResetDevice(device, token).map_err(platform)?;
    }
    Ok(manager)
}

/// Driver profile/format/size evidence complements (never replaces) MFT type admission.
pub(super) fn validate_configuration(
    gpu: &WindowsGpuContext,
    geometry: &picoo_bitstream::VideoSpsFacts,
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

#[cfg(test)]
mod tests;
