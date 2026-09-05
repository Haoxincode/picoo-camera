//! macOS GPUI surface preparation without an intermediate BGRA frame.
//!
//! GPUI currently accepts only `420f` and its Surface shader applies a fixed
//! full-range BT.601 matrix. Picoo's decoded contract is NV12 BT.709 limited,
//! so this adapter scales the two NV12 planes with `fast_image_resize`, applies
//! one direct YCbCr matrix/range transform, then performs the single required
//! copy into a pooled IOSurface-backed CVPixelBuffer (REQ-PICOO-MEDIA-019).

use core_foundation::{
    base::{CFType, TCFType},
    boolean::CFBoolean,
    dictionary::CFDictionary,
    number::CFNumber,
    string::CFString,
};
use core_video::{
    pixel_buffer::{
        kCVPixelBufferHeightKey, kCVPixelBufferMetalCompatibilityKey,
        kCVPixelBufferPixelFormatTypeKey, kCVPixelBufferWidthKey,
        kCVPixelFormatType_420YpCbCr8BiPlanarFullRange, CVPixelBuffer,
    },
    pixel_buffer_io_surface::kCVPixelBufferIOSurfaceCoreAnimationCompatibilityKey,
    pixel_buffer_pool::{
        kCVPixelBufferPoolAllocationThresholdKey, kCVPixelBufferPoolMinimumBufferCountKey,
        CVPixelBufferPool,
    },
    r#return::kCVReturnSuccess,
};
use fast_image_resize::images::{Image, ImageRef};
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};
use picoo_frame_hub::VideoFrame;

const SURFACE_BUFFER_LIMIT: i32 = 3;

pub(super) struct PlatformPreviewResources {
    pool: Option<SurfacePool>,
    resizer: Resizer,
    source_nv12: Vec<u8>,
    prepared_nv12: Vec<u8>,
}

impl Default for PlatformPreviewResources {
    fn default() -> Self {
        Self {
            pool: None,
            resizer: Resizer::new(),
            source_nv12: Vec::new(),
            prepared_nv12: Vec::new(),
        }
    }
}

struct SurfacePool {
    width: u32,
    height: u32,
    pool: CVPixelBufferPool,
    allocation_attributes: CFDictionary<CFString, CFType>,
}

impl PlatformPreviewResources {
    pub(super) fn prepare_surface(
        &mut self,
        frame: &VideoFrame,
        target_width: u32,
    ) -> Option<CVPixelBuffer> {
        validate_frame(frame)?;
        let (width, height) = output_dimensions(frame, target_width)?;
        self.prepare_nv12(frame, width, height)?;
        if self
            .pool
            .as_ref()
            .is_none_or(|pool| pool.width != width || pool.height != height)
        {
            self.pool = SurfacePool::new(width, height);
        }
        self.pool
            .as_ref()?
            .copy_nv12(width, height, &self.prepared_nv12)
    }

    fn prepare_nv12(
        &mut self,
        frame: &VideoFrame,
        output_width: u32,
        output_height: u32,
    ) -> Option<()> {
        if output_width == frame.width && output_height == frame.height {
            // The color-range/matrix adaptation is in-place, so native-size
            // output still needs one owned writable copy.
            copy_tight_nv12(frame, &mut self.prepared_nv12)?;
        } else if frame.stride == frame.width {
            let source_len = (frame.width as usize)
                .checked_mul(frame.height as usize)?
                .checked_mul(3)?
                .checked_div(2)?;
            // Scaling only reads its input; borrow compact decoded storage
            // directly instead of first copying another full NV12 frame.
            resize_nv12_into(
                &mut self.resizer,
                &frame.pixel_data[..source_len],
                &mut self.prepared_nv12,
                frame.width,
                frame.height,
                output_width,
                output_height,
            )?;
        } else {
            copy_tight_nv12(frame, &mut self.source_nv12)?;
            resize_nv12_into(
                &mut self.resizer,
                &self.source_nv12,
                &mut self.prepared_nv12,
                frame.width,
                frame.height,
                output_width,
                output_height,
            )?;
        }
        convert_bt709_limited_to_bt601_full(&mut self.prepared_nv12, output_width, output_height)
    }
}

fn validate_frame(frame: &VideoFrame) -> Option<()> {
    if frame.width == 0
        || frame.height == 0
        || !frame.width.is_multiple_of(2)
        || !frame.height.is_multiple_of(2)
        || frame.stride < frame.width
    {
        return None;
    }
    let y_len = (frame.stride as usize).checked_mul(frame.height as usize)?;
    let uv_len = (frame.stride as usize).checked_mul(frame.height as usize / 2)?;
    (frame.pixel_data.len() >= y_len.checked_add(uv_len)?).then_some(())
}

fn output_dimensions(frame: &VideoFrame, target_width: u32) -> Option<(u32, u32)> {
    if frame.width <= target_width {
        return Some((frame.width, frame.height));
    }
    let width = target_width.min(frame.width) & !1;
    if width < 2 {
        return None;
    }
    let rounded_height = u32::try_from(
        (u64::from(frame.height) * u64::from(width) + u64::from(frame.width) / 2)
            / u64::from(frame.width),
    )
    .ok()?;
    let height = rounded_height.min(frame.height) & !1;
    (height >= 2).then_some((width, height))
}

fn copy_tight_nv12(frame: &VideoFrame, output: &mut Vec<u8>) -> Option<()> {
    let width = frame.width as usize;
    let height = frame.height as usize;
    let stride = frame.stride as usize;
    let source_y_len = stride.checked_mul(height)?;
    let output_len = width.checked_mul(height)?.checked_mul(3)?.checked_div(2)?;
    output.resize(output_len, 0);
    if stride == width {
        output.copy_from_slice(&frame.pixel_data[..output_len]);
        return Some(());
    }

    for (row_index, row) in frame.pixel_data[..source_y_len]
        .chunks_exact(stride)
        .enumerate()
    {
        let target = row_index * width;
        output[target..target + width].copy_from_slice(&row[..width]);
    }
    let uv_source_len = stride.checked_mul(height / 2)?;
    let output_y_len = width * height;
    for (row_index, row) in frame.pixel_data[source_y_len..source_y_len + uv_source_len]
        .chunks_exact(stride)
        .enumerate()
    {
        let target = output_y_len + row_index * width;
        output[target..target + width].copy_from_slice(&row[..width]);
    }
    Some(())
}

fn resize_nv12_into(
    resizer: &mut Resizer,
    source: &[u8],
    output: &mut Vec<u8>,
    source_width: u32,
    source_height: u32,
    output_width: u32,
    output_height: u32,
) -> Option<()> {
    let source_y_len = (source_width as usize).checked_mul(source_height as usize)?;
    let source_y = ImageRef::new(
        source_width,
        source_height,
        &source[..source_y_len],
        PixelType::U8,
    )
    .ok()?;
    let output_y_len = (output_width as usize).checked_mul(output_height as usize)?;
    let output_len = output_y_len.checked_mul(3)?.checked_div(2)?;
    output.resize(output_len, 0);
    let (output_y, output_uv) = output.split_at_mut(output_y_len);
    let mut output_y =
        Image::from_slice_u8(output_width, output_height, output_y, PixelType::U8).ok()?;
    let options = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::CatmullRom));
    resizer
        .resize(&source_y, &mut output_y, Some(&options))
        .ok()?;

    let source_uv = ImageRef::new(
        source_width / 2,
        source_height / 2,
        &source[source_y_len..],
        PixelType::U8x2,
    )
    .ok()?;
    let mut output_uv = Image::from_slice_u8(
        output_width / 2,
        output_height / 2,
        output_uv,
        PixelType::U8x2,
    )
    .ok()?;
    resizer
        .resize(&source_uv, &mut output_uv, Some(&options))
        .ok()?;

    Some(())
}

fn convert_bt709_limited_to_bt601_full(pixels: &mut [u8], width: u32, height: u32) -> Option<()> {
    let width = width as usize;
    let height = height as usize;
    let y_len = width.checked_mul(height)?;
    let required = y_len.checked_mul(3)?.checked_div(2)?;
    if pixels.len() < required || !width.is_multiple_of(2) || !height.is_multiple_of(2) {
        return None;
    }

    // Fixed-point form of BT.709 limited YCbCr -> RGB -> BT.601 full YCbCr.
    // Coefficients use 12 fractional bits and round once per output sample.
    for row in 0..height {
        for column in (0..width).step_by(2) {
            let uv_index = y_len + (row / 2) * width + column;
            let cb = i32::from(pixels[uv_index]) - 128;
            let cr = i32::from(pixels[uv_index + 1]) - 128;
            let chroma_luma = 474 * cb + 914 * cr;
            for offset in 0..2 {
                let y_index = row * width + column + offset;
                let y = i32::from(pixels[y_index]) - 16;
                pixels[y_index] = fixed_to_u8(4_769 * y + chroma_luma);
            }
        }
    }
    for index in (y_len..required).step_by(2) {
        let cb = i32::from(pixels[index]) - 128;
        let cr = i32::from(pixels[index + 1]) - 128;
        pixels[index] = fixed_chroma_to_u8(4_614 * cb - 516 * cr);
        pixels[index + 1] = fixed_chroma_to_u8(-338 * cb + 4_585 * cr);
    }
    Some(())
}

fn fixed_to_u8(value: i32) -> u8 {
    ((value + 2_048) >> 12).clamp(0, 255) as u8
}

fn fixed_chroma_to_u8(value: i32) -> u8 {
    (128 + ((value + 2_048) >> 12)).clamp(0, 255) as u8
}

impl SurfacePool {
    fn new(width: u32, height: u32) -> Option<Self> {
        // SAFETY: these are process-lifetime CoreVideo constant keys.
        let (
            width_key,
            height_key,
            format_key,
            metal_key,
            animation_key,
            minimum_count_key,
            allocation_threshold_key,
        ) = unsafe {
            (
                cf_string(kCVPixelBufferWidthKey),
                cf_string(kCVPixelBufferHeightKey),
                cf_string(kCVPixelBufferPixelFormatTypeKey),
                cf_string(kCVPixelBufferMetalCompatibilityKey),
                cf_string(kCVPixelBufferIOSurfaceCoreAnimationCompatibilityKey),
                cf_string(kCVPixelBufferPoolMinimumBufferCountKey),
                cf_string(kCVPixelBufferPoolAllocationThresholdKey),
            )
        };

        let buffer_attributes = CFDictionary::from_CFType_pairs(&[
            (width_key, CFNumber::from(i64::from(width)).into_CFType()),
            (height_key, CFNumber::from(i64::from(height)).into_CFType()),
            (
                format_key,
                CFNumber::from(i64::from(kCVPixelFormatType_420YpCbCr8BiPlanarFullRange))
                    .into_CFType(),
            ),
            (metal_key, CFBoolean::true_value().clone().into_CFType()),
            (animation_key, CFBoolean::true_value().into_CFType()),
        ]);
        let pool_attributes = CFDictionary::from_CFType_pairs(&[(
            minimum_count_key,
            CFNumber::from(SURFACE_BUFFER_LIMIT).into_CFType(),
        )]);
        let allocation_attributes = CFDictionary::from_CFType_pairs(&[(
            allocation_threshold_key,
            CFNumber::from(SURFACE_BUFFER_LIMIT).into_CFType(),
        )]);
        let pool = CVPixelBufferPool::new(Some(&pool_attributes), Some(&buffer_attributes)).ok()?;

        Some(Self {
            width,
            height,
            pool,
            allocation_attributes,
        })
    }

    fn copy_nv12(&self, width: u32, height: u32, pixels: &[u8]) -> Option<CVPixelBuffer> {
        let pixel_buffer = self
            .pool
            .create_pixel_buffer_with_aux_attributes(Some(&self.allocation_attributes))
            .ok()?;
        if pixel_buffer.lock_base_address(0) != kCVReturnSuccess {
            return None;
        }

        let result = (|| unsafe {
            let width = width as usize;
            let height = height as usize;
            let source_y_len = width.checked_mul(height)?;
            let y_stride = pixel_buffer.get_bytes_per_row_of_plane(0);
            let uv_stride = pixel_buffer.get_bytes_per_row_of_plane(1);
            let y_len = pixel_buffer.get_height_of_plane(0).checked_mul(y_stride)?;
            let uv_len = pixel_buffer.get_height_of_plane(1).checked_mul(uv_stride)?;
            let y_base = pixel_buffer.get_base_address_of_plane(0).cast::<u8>();
            let uv_base = pixel_buffer.get_base_address_of_plane(1).cast::<u8>();
            if y_base.is_null() || uv_base.is_null() {
                return None;
            }
            let y_plane = std::slice::from_raw_parts_mut(y_base, y_len);
            let uv_plane = std::slice::from_raw_parts_mut(uv_base, uv_len);
            copy_rows(&pixels[..source_y_len], width, y_plane, y_stride, height)?;
            copy_rows(
                &pixels[source_y_len..],
                width,
                uv_plane,
                uv_stride,
                height / 2,
            )
        })();
        let unlock_status = pixel_buffer.unlock_base_address(0);
        if result.is_some() && unlock_status == kCVReturnSuccess {
            Some(pixel_buffer)
        } else {
            None
        }
    }
}

fn copy_rows(
    source: &[u8],
    source_stride: usize,
    target: &mut [u8],
    target_stride: usize,
    rows: usize,
) -> Option<()> {
    if target_stride < source_stride
        || source.len() < source_stride.checked_mul(rows)?
        || target.len() < target_stride.checked_mul(rows)?
    {
        return None;
    }
    for row in 0..rows {
        let source_offset = row * source_stride;
        let target_offset = row * target_stride;
        target[target_offset..target_offset + source_stride]
            .copy_from_slice(&source[source_offset..source_offset + source_stride]);
    }
    Some(())
}

unsafe fn cf_string(value: core_foundation::string::CFStringRef) -> CFString {
    // SAFETY: caller guarantees a live Core Foundation string constant.
    unsafe { CFString::wrap_under_get_rule(value) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use std::time::Instant;

    fn frame(width: u32, height: u32, pixels: Vec<u8>) -> VideoFrame {
        VideoFrame::new(
            1,
            1,
            0,
            0,
            0,
            0,
            Instant::now(),
            0,
            width,
            height,
            width,
            0,
            Bytes::from(pixels),
        )
    }

    #[test]
    fn black_and_white_preserve_full_range_endpoints() {
        let mut pixels = vec![16; 6];
        pixels[4..].fill(128);
        convert_bt709_limited_to_bt601_full(&mut pixels, 2, 2).expect("black");
        assert_eq!(&pixels[..4], &[0, 0, 0, 0]);
        assert_eq!(&pixels[4..], &[128, 128]);

        pixels[..4].fill(235);
        convert_bt709_limited_to_bt601_full(&mut pixels, 2, 2).expect("white");
        assert_eq!(&pixels[..4], &[255, 255, 255, 255]);
    }

    #[test]
    fn bt709_red_maps_to_bt601_full_without_bgra() {
        let mut pixels = vec![81, 81, 81, 81, 90, 240];
        convert_bt709_limited_to_bt601_full(&mut pixels, 2, 2).expect("red");
        assert!(pixels[..4].iter().all(|&y| (92..=100).contains(&y)));
        assert!((68..=75).contains(&pixels[4]));
        assert!(pixels[5] >= 250);
    }

    #[test]
    fn resize_keeps_even_nv12_dimensions_and_plane_size() {
        let source = frame(1920, 1080, vec![16; 1920 * 1080 * 3 / 2]);
        let mut resources = PlatformPreviewResources::default();
        let (width, height) = output_dimensions(&source, 1441).expect("dimensions");
        resources
            .prepare_nv12(&source, width, height)
            .expect("prepare");
        assert_eq!((width, height), (1440, 810));
        assert_eq!(resources.prepared_nv12.len(), 1440 * 810 * 3 / 2);
    }

    #[test]
    fn surface_pool_is_hard_bounded_and_recycles() {
        let source = frame(2, 2, vec![16, 16, 16, 16, 128, 128]);
        let mut resources = PlatformPreviewResources::default();
        let first = resources
            .prepare_surface(&source, 2)
            .expect("first surface");
        let second = resources
            .prepare_surface(&source, 2)
            .expect("second surface");
        let third = resources
            .prepare_surface(&source, 2)
            .expect("third surface");

        assert_eq!(first.get_width(), 2);
        assert_eq!(first.get_height(), 2);
        assert_eq!(
            first.get_pixel_format(),
            kCVPixelFormatType_420YpCbCr8BiPlanarFullRange
        );
        assert!(resources.prepare_surface(&source, 2).is_none());

        drop(first);
        assert!(resources.prepare_surface(&source, 2).is_some());
        drop((second, third));
    }
}
