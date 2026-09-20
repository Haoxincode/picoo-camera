//! Original coded geometry plus actual CoreVideo output presentation.
//! REQ-PICOO-NEXT-011/025: metadata reads only; never map a source plane.
use crate::{DecodeError, NativeDecodedFormat};
use objc2_core_video::*;
use picoo_bitstream::VideoSpsFacts;
use picoo_frame_hub::{
    remaining_visible_after_decoder, ChromaSiting, ImageSize, NativeImage, PixelAspectRatio,
    VisibleRect,
};

pub(crate) use crate::source_format::validate_source;

pub(crate) fn describe(
    facts: &VideoSpsFacts,
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
        let actual = unsafe { buffer.attachment(key, std::ptr::null_mut()) };
        if actual.as_deref() != Some(expected.as_ref()) {
            return Err(DecodeError::Platform(format!(
                "native decoder output lacks explicit BT.709 color ({key:?} got {actual:?})"
            )));
        }
    }
    let clean = CVImageBufferGetCleanRect(buffer);
    let display = CVImageBufferGetDisplaySize(buffer);
    let integer = |value: f64| -> Result<u32, DecodeError> {
        if !value.is_finite() || value < 0.0 || value > u32::MAX as f64 || value.fract() != 0.0 {
            return Err(DecodeError::Platform(format!(
                "invalid native clean aperture {value}"
            )));
        }
        Ok(value as u32)
    };
    let platform = VisibleRect {
        x: integer(clean.origin.x)?,
        // CoreVideo reports lower-left coordinates; source metadata is top-left.
        y: integer(f64::from(image.height()) - clean.origin.y - clean.size.height)?,
        width: integer(clean.size.width)?,
        height: integer(clean.size.height)?,
    };
    let admitted = VisibleRect {
        x: facts.visible_x,
        y: facts.visible_y,
        width: facts.visible_width,
        height: facts.visible_height,
    };
    let coded = ImageSize {
        width: facts.coded_width,
        height: facts.coded_height,
    };
    let allocation = ImageSize {
        width: image.width(),
        height: image.height(),
    };
    let visible_rect = remaining_visible_after_decoder(admitted, coded, allocation, platform)
        .ok_or_else(|| {
            DecodeError::Platform(format!(
                "native output geometry conflicts with admitted source: image={}x{} clean={}x{}+{}+{} display={}x{} coded={}x{} visible={}x{}+{}+{}",
                allocation.width,
                allocation.height,
                platform.width,
                platform.height,
                platform.x,
                platform.y,
                display.width,
                display.height,
                coded.width,
                coded.height,
                admitted.width,
                admitted.height,
                admitted.x,
                admitted.y,
            ))
        })?;
    let display_w = integer(display.width)?;
    let display_h = integer(display.height)?;
    let display_matches_remaining =
        display_w == visible_rect.width && display_h == visible_rect.height;
    let display_matches_coded_allocation = visible_rect == admitted
        && platform
            == (VisibleRect {
                x: 0,
                y: 0,
                width: allocation.width,
                height: allocation.height,
            })
        && display_w == allocation.width
        && display_h == allocation.height;
    if !display_matches_remaining && !display_matches_coded_allocation {
        return Err(DecodeError::Platform(format!(
            "native output display size is not square pixels of the remaining picture: display={display_w}x{display_h} remaining={}x{}+{}+{}",
            visible_rect.width, visible_rect.height, visible_rect.x, visible_rect.y
        )));
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

#[cfg(test)]
mod tests {
    use super::*;
    use picoo_bitstream::VideoSpsFacts;

    fn facts(
        coded_width: u32,
        coded_height: u32,
        visible_width: u32,
        visible_height: u32,
    ) -> VideoSpsFacts {
        VideoSpsFacts {
            coded_width,
            coded_height,
            visible_x: 0,
            visible_y: 0,
            visible_width,
            visible_height,
            pixel_aspect_ratio: Some((1, 1)),
            color: None,
            chroma_location: 0,
        }
    }

    fn nv12(width: u32, height: u32) -> NativeImage {
        crate::native_fixture::upload(
            width,
            height,
            width,
            &vec![128; (width * height * 3 / 2) as usize],
        )
        .unwrap()
    }

    #[test]
    fn coded_allocation_keeps_admitted_remaining_crop() {
        let described = describe(&facts(192, 96, 192, 88), &nv12(192, 96)).unwrap();
        assert_eq!(
            described.visible_rect,
            VisibleRect {
                x: 0,
                y: 0,
                width: 192,
                height: 88
            }
        );
        assert_eq!(
            described.coded_size,
            ImageSize {
                width: 192,
                height: 96
            }
        );
    }

    #[test]
    fn cropped_allocation_needs_no_further_sps_crop() {
        let described = describe(&facts(192, 96, 192, 88), &nv12(192, 88)).unwrap();
        assert_eq!(
            described.visible_rect,
            VisibleRect {
                x: 0,
                y: 0,
                width: 192,
                height: 88
            }
        );
    }

    #[test]
    fn allocation_that_is_neither_coded_nor_visible_is_rejected() {
        assert!(describe(&facts(192, 96, 192, 88), &nv12(128, 72)).is_err());
    }
}
