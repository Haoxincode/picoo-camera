use std::ptr::{self, NonNull};

use objc2_core_foundation::{CFBoolean, CFDictionary, CFNumber, CFRetained, CFString, CFType};
use objc2_core_video::*;

use crate::{OutputColor, RenderError, RenderSpec};

/// One fixed output layout and a hard three-buffer allocation threshold.
/// A retained output keeps its allocation unavailable to this pool.
pub(super) struct OutputPool {
    pool: CFRetained<CVPixelBufferPool>,
    allocation: CFRetained<CFDictionary<CFString, CFNumber>>,
}

impl OutputPool {
    pub fn new(spec: RenderSpec) -> Result<Self, RenderError> {
        spec.validate()?;
        if spec.format != crate::OutputFormat::Nv12 {
            return Err(RenderError::UnsupportedOutputFormat);
        }
        let width = CFNumber::new_i32(spec.width as i32);
        let height = CFNumber::new_i32(spec.height as i32);
        let format = CFNumber::new_i64(match spec.color {
            OutputColor::Bt601Full => kCVPixelFormatType_420YpCbCr8BiPlanarFullRange as i64,
            OutputColor::Bt709Limited => kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange as i64,
            OutputColor::RgbFullG22Bt709 => return Err(RenderError::UnsupportedOutputFormat),
        });
        let surface = CFDictionary::<CFString, CFType>::empty();
        let metal = CFBoolean::new(true);
        // SAFETY: Framework constants and their documented value types.
        let attributes = unsafe {
            CFDictionary::<CFString, CFType>::from_slices(
                &[
                    kCVPixelBufferWidthKey,
                    kCVPixelBufferHeightKey,
                    kCVPixelBufferPixelFormatTypeKey,
                    kCVPixelBufferIOSurfacePropertiesKey,
                    kCVPixelBufferMetalCompatibilityKey,
                ],
                &[
                    width.as_ref(),
                    height.as_ref(),
                    format.as_ref(),
                    surface.as_ref(),
                    metal.as_ref(),
                ],
            )
        };
        let mut raw = ptr::null_mut();
        // SAFETY: Valid dictionaries and live output pointer; Create returns +1.
        let status = unsafe {
            CVPixelBufferPool::create(
                None,
                None,
                Some(attributes.as_opaque()),
                NonNull::from(&mut raw),
            )
        };
        if status != 0 {
            return Err(RenderError::Platform(format!(
                "CVPixelBufferPoolCreate: {status}"
            )));
        }
        let raw =
            NonNull::new(raw).ok_or_else(|| RenderError::Platform("null output pool".into()))?;
        let limit = CFNumber::new_i32(3);
        let allocation = CFDictionary::from_slices(
            &[unsafe { kCVPixelBufferPoolAllocationThresholdKey }],
            &[&*limit],
        );
        Ok(Self {
            pool: unsafe { CFRetained::from_raw(raw) },
            allocation,
        })
    }

    pub fn acquire(&mut self) -> Result<CFRetained<CVPixelBuffer>, RenderError> {
        let mut raw = ptr::null_mut();
        // SAFETY: Correct numeric threshold, exclusive worker, live output slot.
        let status = unsafe {
            CVPixelBufferPool::create_pixel_buffer_with_aux_attributes(
                None,
                &self.pool,
                Some(self.allocation.as_opaque()),
                NonNull::from(&mut raw),
            )
        };
        if status == kCVReturnWouldExceedAllocationThreshold {
            return Err(RenderError::PoolFull);
        }
        if status != 0 {
            return Err(RenderError::Platform(format!(
                "CVPixelBufferPool allocation: {status}"
            )));
        }
        let raw =
            NonNull::new(raw).ok_or_else(|| RenderError::Platform("null output image".into()))?;
        // SAFETY: Successful allocation returns +1 ownership.
        Ok(unsafe { CFRetained::from_raw(raw) })
    }
}
