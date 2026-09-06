use crate::{RenderError, RenderSpec, Rotation};
use picoo_frame_hub::ImageSize;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709, DXGI_FORMAT_NV12, DXGI_RATIONAL,
};

pub(super) struct Pipeline {
    pub size: ImageSize,
    pub enumerator: ID3D11VideoProcessorEnumerator,
    pub processor: ID3D11VideoProcessor,
}
impl Pipeline {
    pub(super) unsafe fn new(
        device: &ID3D11VideoDevice,
        size: ImageSize,
        spec: RenderSpec,
    ) -> Result<Self, RenderError> {
        let enumerator = device
            .CreateVideoProcessorEnumerator(&D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
                InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
                InputFrameRate: DXGI_RATIONAL {
                    Numerator: 60,
                    Denominator: 1,
                },
                InputWidth: size.width,
                InputHeight: size.height,
                OutputFrameRate: DXGI_RATIONAL {
                    Numerator: 60,
                    Denominator: 1,
                },
                OutputWidth: spec.width,
                OutputHeight: spec.height,
                Usage: D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
            })
            .map_err(super::platform)?;
        let support = enumerator
            .CheckVideoProcessorFormat(DXGI_FORMAT_NV12)
            .map_err(super::platform)?;
        let required = (D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_INPUT.0
            | D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_OUTPUT.0) as u32;
        if support & required != required {
            return Err(super::unsupported("NV12 input/output"));
        }
        let extended: ID3D11VideoProcessorEnumerator1 =
            enumerator.cast().map_err(super::platform)?;
        if !extended
            .CheckVideoProcessorFormatConversion(
                DXGI_FORMAT_NV12,
                DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709,
                DXGI_FORMAT_NV12,
                DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709,
            )
            .map_err(super::platform)?
            .as_bool()
        {
            return Err(super::unsupported("BT.709 limited color contract"));
        }
        let mut caps = D3D11_VIDEO_PROCESSOR_CAPS::default();
        enumerator
            .GetVideoProcessorCaps(&mut caps)
            .map_err(super::platform)?;
        if spec.rotation != Rotation::None
            && caps.FeatureCaps & D3D11_VIDEO_PROCESSOR_FEATURE_CAPS_ROTATION.0 as u32 == 0
        {
            return Err(super::unsupported("rotation"));
        }
        if spec.mirror && caps.FeatureCaps & D3D11_VIDEO_PROCESSOR_FEATURE_CAPS_MIRROR.0 as u32 == 0
        {
            return Err(super::unsupported("mirror"));
        }
        for index in 0..caps.RateConversionCapsCount.min(32) {
            let mut rate = D3D11_VIDEO_PROCESSOR_RATE_CONVERSION_CAPS::default();
            enumerator
                .GetVideoProcessorRateConversionCaps(index, &mut rate)
                .map_err(super::platform)?;
            if rate.PastFrames == 0 && rate.FutureFrames == 0 {
                let processor = device
                    .CreateVideoProcessor(&enumerator, index)
                    .map_err(super::platform)?;
                return Ok(Self {
                    size,
                    enumerator,
                    processor,
                });
            }
        }
        Err(super::unsupported("stateless progressive processing"))
    }
}
