//! Original coded geometry plus actual CoreVideo output presentation.
//! REQ-PICOO-NEXT-011/025: metadata reads only; never map a source plane.
use crate::{DecodeError, NativeDecodedFormat};
use objc2_core_video::*;
use picoo_bitstream::AvcSpsFacts;
use picoo_frame_hub::{ChromaSiting, ImageSize, NativeImage, PixelAspectRatio, VisibleRect};

pub(crate) fn validate_source(facts: &AvcSpsFacts) -> Result<(), DecodeError> {
    if facts.pixel_aspect_ratio.is_some_and(|(w, h)| w != h)
        || facts.chroma_location > 1
        || facts.color.is_some_and(|color| {
            color.full_range
                || !matches!(color.primaries, 1 | 2)
                || !matches!(color.transfer, 1 | 2)
                || !matches!(color.matrix, 1 | 2)
        })
    {
        return Err(DecodeError::Platform(
            "unsupported native AVC presentation or color".into(),
        ));
    }
    Ok(())
}

pub(crate) fn describe(
    facts: &AvcSpsFacts,
    image: &NativeImage,
) -> Result<NativeDecodedFormat, DecodeError> {
    let lease = image
        .apple()
        .ok_or_else(|| DecodeError::Platform("missing Apple native image".into()))?;
    // SAFETY: Read-only metadata on the retained completed decode output.
    let buffer = unsafe { lease.pixel_buffer() };
    for (key, expected) in unsafe {
        [
            (
                kCVImageBufferColorPrimariesKey,
                kCVImageBufferColorPrimaries_ITU_R_709_2,
            ),
            (
                kCVImageBufferTransferFunctionKey,
                kCVImageBufferTransferFunction_ITU_R_709_2,
            ),
            (
                kCVImageBufferYCbCrMatrixKey,
                kCVImageBufferYCbCrMatrix_ITU_R_709_2,
            ),
        ]
    } {
        // SAFETY: Framework key and immutable retained buffer; attachment mode is optional.
        if unsafe { buffer.attachment(key, std::ptr::null_mut()) }.as_deref()
            != Some(expected.as_ref())
        {
            return Err(DecodeError::Platform(
                "native decoder output lacks explicit BT.709 color".into(),
            ));
        }
    }
    let clean = CVImageBufferGetCleanRect(buffer);
    let display = CVImageBufferGetDisplaySize(buffer);
    let integer = |value: f64| -> Result<u32, DecodeError> {
        if !value.is_finite() || value < 0.0 || value > u32::MAX as f64 || value.fract() != 0.0 {
            return Err(DecodeError::Platform(
                "invalid native clean aperture".into(),
            ));
        }
        Ok(value as u32)
    };
    let visible_rect = VisibleRect {
        x: integer(clean.origin.x)?,
        // CoreVideo reports lower-left coordinates; source metadata is top-left.
        y: integer(f64::from(image.height()) - clean.origin.y - clean.size.height)?,
        width: integer(clean.size.width)?,
        height: integer(clean.size.height)?,
    };
    if (visible_rect.width, visible_rect.height) != (facts.visible_width, facts.visible_height)
        || visible_rect
            .x
            .checked_add(visible_rect.width)
            .is_none_or(|n| n > image.width())
        || visible_rect
            .y
            .checked_add(visible_rect.height)
            .is_none_or(|n| n > image.height())
        || display.width != clean.size.width
        || display.height != clean.size.height
    {
        return Err(DecodeError::Platform(
            "native output geometry differs from the admitted square-pixel source".into(),
        ));
    }
    // Native output PAR is established by CoreVideo's nominal display size in
    // square pixels, not inferred from an absent SPS aspect_ratio_info field.
    Ok(NativeDecodedFormat {
        coded_size: ImageSize {
            width: facts.coded_width,
            height: facts.coded_height,
        },
        visible_rect,
        pixel_aspect_ratio: PixelAspectRatio {
            numerator: 1,
            denominator: 1,
        },
        chroma_siting: if facts.chroma_location == 0 {
            ChromaSiting::Left
        } else {
            ChromaSiting::Center
        },
    })
}
