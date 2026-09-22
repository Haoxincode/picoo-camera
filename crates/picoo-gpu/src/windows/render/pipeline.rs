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
        if support & D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_INPUT.0 as u32 == 0 {
            return Err(super::unsupported("NV12 input"));
        }
        let output = super::output_format(spec);
        if enumerator
            .CheckVideoProcessorFormat(output)
            .map_err(super::platform)?
            & D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_OUTPUT.0 as u32
            == 0
        {
            return Err(super::unsupported("requested target storage"));
        }
        let extended: ID3D11VideoProcessorEnumerator1 =
            enumerator.cast().map_err(super::platform)?;
        if !extended
            .CheckVideoProcessorFormatConversion(
                DXGI_FORMAT_NV12,
                DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709,
                output,
                super::output_color(spec),
            )
            .map_err(super::platform)?
            .as_bool()
        {
            return Err(super::unsupported(
                "requested source-to-target color conversion",
            ));
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
        let index = progressive_processor_index(caps.RateConversionCapsCount, |index| {
            let mut rate = D3D11_VIDEO_PROCESSOR_RATE_CONVERSION_CAPS::default();
            enumerator
                .GetVideoProcessorRateConversionCaps(index, &mut rate)
                .map_err(super::platform)?;
            Ok(rate)
        })?;
        let processor = device
            .CreateVideoProcessor(&enumerator, index)
            .map_err(super::platform)?;
        Ok(Self {
            size,
            enumerator,
            processor,
        })
    }
}

// REQ-PICOO-GPU-006: reference counts describe OPTIMAL temporal processing,
// not a minimum for progressive color/geometry conversion. Prefer the least
// temporal capability, but admit nonzero counts. Each Blt still supplies only
// the current progressive frame, with normal output rate and auto processing off.
fn progressive_processor_index(
    count: u32,
    mut query: impl FnMut(u32) -> Result<D3D11_VIDEO_PROCESSOR_RATE_CONVERSION_CAPS, RenderError>,
) -> Result<u32, RenderError> {
    let mut selected = None;
    for index in 0..count {
        let rate = query(index)?;
        let preference = (rate.FutureFrames, rate.PastFrames, index);
        selected = Some(selected.map_or(preference, |current| preference.min(current)));
    }
    selected
        .map(|(_, _, index)| index)
        .ok_or_else(|| super::unsupported("any rate-conversion processor"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn select(references: &[(u32, u32)]) -> Result<u32, RenderError> {
        progressive_processor_index(references.len() as u32, |index| {
            let (past, future) = references[index as usize];
            Ok(D3D11_VIDEO_PROCESSOR_RATE_CONVERSION_CAPS {
                PastFrames: past,
                FutureFrames: future,
                ..Default::default()
            })
        })
    }

    #[test]
    fn progressive_conversion_accepts_groups_that_all_advertise_reference_frames() {
        // Regression: build 640 rejected every frame on Intel Iris Xe before Blt.
        // These are representative nonzero capabilities, not a hardware dump.
        assert_eq!(select(&[(1, 0)]).unwrap(), 0);
        assert_eq!(select(&[(2, 1), (1, 0), (4, 2)]).unwrap(), 1);
        assert_eq!(select(&[(2, 1)]).unwrap(), 0);
    }

    #[test]
    fn progressive_conversion_prefers_less_history_without_hiding_query_errors() {
        assert_eq!(select(&[(2, 1), (0, 0), (0, 0)]).unwrap(), 1);
        assert!(select(&[]).is_err());
        assert!(matches!(
            progressive_processor_index(1, |_| Err(RenderError::DeviceUnavailable)),
            Err(RenderError::DeviceUnavailable)
        ));
    }
}
