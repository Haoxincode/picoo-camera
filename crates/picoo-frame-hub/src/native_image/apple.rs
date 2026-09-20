//! Immutable Apple source image ownership — REQ-PICOO-NEXT-009/016/029.

use std::ptr::NonNull;

use objc2_core_foundation::CFRetained;
use objc2_core_video::{
    kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange, CVPixelBuffer, CVPixelBufferGetHeight,
    CVPixelBufferGetIOSurface, CVPixelBufferGetPixelFormatType, CVPixelBufferGetPlaneCount,
    CVPixelBufferGetWidth,
};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum NativeImageError {
    #[error("native source image must be an IOSurface-backed limited-range NV12 buffer")]
    UnsupportedStorage,
    #[error("native source image has invalid dimensions")]
    InvalidDimensions,
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_core_foundation::{CFDictionary, CFString, CFType};
    use objc2_core_video::{
        kCVPixelBufferIOSurfacePropertiesKey, kCVPixelFormatType_32BGRA, CVPixelBufferCreate,
    };

    fn buffer(format: u32, surface: bool) -> CFRetained<CVPixelBuffer> {
        let properties = CFDictionary::<CFString, CFType>::empty();
        let attributes = CFDictionary::from_slices(
            &[unsafe { kCVPixelBufferIOSurfacePropertiesKey }],
            &[&*properties],
        );
        let mut raw = std::ptr::null_mut();
        // SAFETY: Correctly typed IOSurface dictionary and live output pointer.
        let status = unsafe {
            CVPixelBufferCreate(
                None,
                1280,
                720,
                format,
                surface.then(|| attributes.as_opaque()),
                NonNull::from(&mut raw),
            )
        };
        assert_eq!(status, 0);
        // SAFETY: Successful Create returns a +1 owned object.
        unsafe { CFRetained::from_raw(NonNull::new(raw).unwrap()) }
    }

    #[test]
    fn retained_surface_survives_callback_and_cross_thread_handoff() {
        let buffer = buffer(kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange, true);
        let original = NonNull::from(&*buffer).as_ptr() as usize;
        // SAFETY: This test never writes the buffer or its attachments.
        let image = unsafe { ApplePixelBufferLease::retain_completed(&buffer) }.unwrap();
        drop(buffer);
        let consumer = image.clone();
        drop(image);
        std::thread::spawn(move || {
            assert_eq!((consumer.width(), consumer.height()), (1280, 720));
            // SAFETY: Read-only identity inspection, no asynchronous GPU work.
            let retained = unsafe { consumer.pixel_buffer() };
            assert_eq!(NonNull::from(retained).as_ptr() as usize, original);
            assert!(CVPixelBufferGetIOSurface(Some(retained)).is_some());
        })
        .join()
        .unwrap();
    }

    #[test]
    fn rejects_cpu_allocation_and_wrong_pixel_format() {
        for buffer in [
            buffer(kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange, false),
            buffer(kCVPixelFormatType_32BGRA, true),
        ] {
            // SAFETY: No writes or mutable aliases in this test.
            assert_eq!(
                unsafe { ApplePixelBufferLease::retain_completed(&buffer) }.unwrap_err(),
                NativeImageError::UnsupportedStorage,
            );
        }
    }
}

/// Retains a completed, immutable native source image without mapping its pixels.
///
/// Cloning retains the same allocation. This is not a GPU completion signal:
/// consumers must hold a clone until every submitted GPU read has completed.
/// The platform pool, not this type, decides when the allocation can be reused.
#[derive(Clone)]
pub struct ApplePixelBufferLease {
    buffer: CFRetained<CVPixelBuffer>,
    width: u32,
    height: u32,
}

impl std::fmt::Debug for ApplePixelBufferLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApplePixelBufferLease")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

// SAFETY: CoreVideo retention/release is thread-safe. The unsafe constructor
// requires completed writes and immutability through all aliases. Safe methods
// expose dimensions only, never mutable pixels or attachments. GPU consumers
// separately retain this owner until their completion fence is signalled.
unsafe impl Send for ApplePixelBufferLease {}
unsafe impl Sync for ApplePixelBufferLease {}

impl ApplePixelBufferLease {
    /// Retains a platform decoder's completed output before its callback returns.
    ///
    /// # Safety
    /// All writes to pixels and attachments must have completed. The producer
    /// and all aliases must leave them immutable while any image clone exists.
    /// In particular, the producer must respect CoreVideo pool retention rather
    /// than manually overwrite a retained allocation.
    pub unsafe fn retain_completed(buffer: &CVPixelBuffer) -> Result<Self, NativeImageError> {
        if CVPixelBufferGetPixelFormatType(buffer)
            != kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
            || CVPixelBufferGetPlaneCount(buffer) != 2
            || CVPixelBufferGetIOSurface(Some(buffer)).is_none()
        {
            return Err(NativeImageError::UnsupportedStorage);
        }
        let width = u32::try_from(CVPixelBufferGetWidth(buffer))
            .map_err(|_| NativeImageError::InvalidDimensions)?;
        let height = u32::try_from(CVPixelBufferGetHeight(buffer))
            .map_err(|_| NativeImageError::InvalidDimensions)?;
        if width == 0 || height == 0 || width % 2 != 0 || height % 2 != 0 {
            return Err(NativeImageError::InvalidDimensions);
        }
        Ok(Self {
            // SAFETY: The borrowed buffer is live for this retain operation.
            buffer: unsafe { CFRetained::retain(NonNull::from(buffer)) },
            width,
            height,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Borrows the native object for a platform GPU adapter.
    ///
    /// # Safety
    /// The caller must not write pixels or attachments, nor expose a mutable
    /// alias. A GPU command referencing this buffer must retain an image clone
    /// until completion, including cancellation and error paths.
    pub unsafe fn pixel_buffer(&self) -> &CVPixelBuffer {
        &self.buffer
    }
}
