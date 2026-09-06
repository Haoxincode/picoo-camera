//! Diagnostic source upload only; excluded from all normal product factories.

use crate::DecodeError;
use objc2_core_foundation::{CFDictionary, CFRetained, CFString, CFType};
use objc2_core_video::*;
use picoo_frame_hub::{ApplePixelBufferLease, NativeImage};
use std::ptr::{self, NonNull};

pub(super) fn upload(
    width: u32,
    height: u32,
    stride: u32,
    pixels: &[u8],
) -> Result<NativeImage, DecodeError> {
    let (width, height, stride) = (width as usize, height as usize, stride as usize);
    if width == 0
        || height == 0
        || !width.is_multiple_of(2)
        || !height.is_multiple_of(2)
        || stride < width
        || stride
            .checked_mul(height)
            .and_then(|y| y.checked_add(y / 2))
            .is_none_or(|length| length > pixels.len())
    {
        return Err(DecodeError::Platform(
            "invalid diagnostic NV12 fixture".into(),
        ));
    }
    let surface = CFDictionary::<CFString, CFType>::empty();
    let attributes = CFDictionary::from_slices(
        &[unsafe { kCVPixelBufferIOSurfacePropertiesKey }],
        &[&*surface],
    );
    let mut raw = ptr::null_mut();
    // SAFETY: Correct IOSurface attributes and a valid Create output pointer.
    let status = unsafe {
        CVPixelBufferCreate(
            None,
            width,
            height,
            kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
            Some(attributes.as_opaque()),
            NonNull::from(&mut raw),
        )
    };
    if status != 0 {
        return Err(DecodeError::Platform(format!(
            "diagnostic CVPixelBufferCreate: {status}"
        )));
    }
    let raw =
        NonNull::new(raw).ok_or_else(|| DecodeError::Platform("null diagnostic image".into()))?;
    let buffer = unsafe { CFRetained::from_raw(raw) };
    // This writable mapping is solely for an explicit diagnostic fixture.
    unsafe {
        let status = CVPixelBufferLockBaseAddress(&buffer, CVPixelBufferLockFlags::empty());
        if status != 0 {
            return Err(DecodeError::Platform(format!(
                "diagnostic image lock: {status}"
            )));
        }
        for plane in 0..2 {
            let rows = if plane == 0 { height } else { height / 2 };
            let source_offset = if plane == 0 { 0 } else { stride * height };
            let destination_stride = CVPixelBufferGetBytesPerRowOfPlane(&buffer, plane);
            let destination = CVPixelBufferGetBaseAddressOfPlane(&buffer, plane).cast::<u8>();
            if destination.is_null() || destination_stride < width {
                CVPixelBufferUnlockBaseAddress(&buffer, CVPixelBufferLockFlags::empty());
                return Err(DecodeError::Platform(
                    "invalid diagnostic image plane".into(),
                ));
            }
            for row in 0..rows {
                ptr::copy_nonoverlapping(
                    pixels[source_offset + row * stride..].as_ptr(),
                    destination.add(row * destination_stride),
                    width,
                );
            }
        }
        let status = CVPixelBufferUnlockBaseAddress(&buffer, CVPixelBufferLockFlags::empty());
        if status != 0 {
            return Err(DecodeError::Platform(format!(
                "diagnostic image unlock: {status}"
            )));
        }
        for (key, value) in [
            (
                kCVImageBufferYCbCrMatrixKey,
                kCVImageBufferYCbCrMatrix_ITU_R_709_2,
            ),
            (
                kCVImageBufferColorPrimariesKey,
                kCVImageBufferColorPrimaries_ITU_R_709_2,
            ),
            (
                kCVImageBufferTransferFunctionKey,
                kCVImageBufferTransferFunction_ITU_R_709_2,
            ),
        ] {
            buffer.set_attachment(key, value.as_ref(), CVAttachmentMode::ShouldPropagate);
        }
        ApplePixelBufferLease::retain_completed(&buffer)
            .map(NativeImage::Apple)
            .map_err(|error| DecodeError::Platform(error.to_string()))
    }
}
