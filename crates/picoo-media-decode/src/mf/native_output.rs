//! Original completed MF sample ownership plus actual negotiated presentation.
//! REQ-PICOO-NEXT-011/025: never Lock, Map, transform, or export a source image.
use super::runtime::MfRuntimeGuard;
use crate::{DecodeError, DecodedFrame, NativeDecodedFormat};
use picoo_bitstream::VideoSpsFacts;
use picoo_frame_hub::{
    ChromaSiting, D3D11ImageLease, ImageSize, NativeImage, PixelAspectRatio, VisibleRect,
};
use picoo_gpu::WindowsGpuContext;
use windows::Win32::Media::MediaFoundation::*;

fn platform(error: windows::core::Error) -> DecodeError {
    DecodeError::Platform(format!("native MF output description: {error}"))
}

pub(super) unsafe fn sample_to_frame(
    sample: &IMFSample,
    transform: &IMFTransform,
    facts: &VideoSpsFacts,
    gpu: &WindowsGpuContext,
    runtime: &MfRuntimeGuard,
) -> Result<DecodedFrame, DecodeError> {
    let image = NativeImage::Windows(
        D3D11ImageLease::retain_completed(sample, gpu.device(), runtime._lifetime.clone())
            .map_err(|e| DecodeError::Platform(e.to_string()))?,
    );
    let media = transform.GetOutputCurrentType(0).map_err(platform)?;
    let format = describe(&media, facts, &image)?;
    Ok(DecodedFrame::native(
        image,
        format,
        0,
        crate::now_timestamp_us(),
    ))
}

unsafe fn describe(
    media: &IMFMediaType,
    facts: &VideoSpsFacts,
    image: &NativeImage,
) -> Result<NativeDecodedFormat, DecodeError> {
    if media.GetGUID(&MF_MT_SUBTYPE).map_err(platform)? != MFVideoFormat_NV12 {
        return Err(DecodeError::ConfigurationMismatch);
    }
    for (key, expected) in [
        (MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32),
        (MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0 as u32),
        (MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0 as u32),
        (MF_MT_VIDEO_PRIMARIES, MFVideoPrimaries_BT709.0 as u32),
        (MF_MT_TRANSFER_FUNCTION, MFVideoTransFunc_709.0 as u32),
    ] {
        if media.GetUINT32(&key).map_err(platform)? != expected {
            return Err(DecodeError::ConfigurationMismatch);
        }
    }
    let size = media.GetUINT64(&MF_MT_FRAME_SIZE).map_err(platform)?;
    let width = (size >> 32) as u32;
    let height = size as u32;
    let par = media
        .GetUINT64(&MF_MT_PIXEL_ASPECT_RATIO)
        .map_err(platform)?;
    let numerator = (par >> 32) as u32;
    let denominator = par as u32;
    if width == 0
        || height == 0
        || width > image.width()
        || height > image.height()
        || numerator == 0
        || numerator != denominator
    {
        return Err(DecodeError::ConfigurationMismatch);
    }
    let mut area = MFVideoArea::default();
    let bytes = std::slice::from_raw_parts_mut(
        (&mut area as *mut MFVideoArea).cast::<u8>(),
        std::mem::size_of::<MFVideoArea>(),
    );
    let mut written = 0;
    let rect = match media.GetBlob(&MF_MT_MINIMUM_DISPLAY_APERTURE, bytes, Some(&mut written)) {
        Ok(())
            if written as usize == std::mem::size_of::<MFVideoArea>()
                && area.OffsetX.fract == 0
                && area.OffsetY.fract == 0
                && area.OffsetX.value >= 0
                && area.OffsetY.value >= 0
                && area.Area.cx > 0
                && area.Area.cy > 0 =>
        {
            VisibleRect {
                x: area.OffsetX.value as u32,
                y: area.OffsetY.value as u32,
                width: area.Area.cx as u32,
                height: area.Area.cy as u32,
            }
        }
        Err(error) if error.code() == MF_E_ATTRIBUTENOTFOUND => VisibleRect {
            x: 0,
            y: 0,
            width,
            height,
        },
        Err(error) => return Err(platform(error)),
        _ => return Err(DecodeError::ConfigurationMismatch),
    };
    if (rect.width, rect.height) != (facts.visible_width, facts.visible_height)
        || rect
            .x
            .checked_add(rect.width)
            .is_none_or(|right| right > width)
        || rect
            .y
            .checked_add(rect.height)
            .is_none_or(|bottom| bottom > height)
        || !rect.x.is_multiple_of(2)
        || !rect.y.is_multiple_of(2)
        || facts.chroma_location != 0
    {
        return Err(DecodeError::ConfigurationMismatch);
    }
    Ok(NativeDecodedFormat {
        coded_size: ImageSize {
            width: facts.coded_width,
            height: facts.coded_height,
        },
        visible_rect: rect,
        pixel_aspect_ratio: PixelAspectRatio {
            numerator,
            denominator,
        },
        chroma_siting: ChromaSiting::Left,
    })
}

#[cfg(test)]
mod tests;
