//! Fused NV12 orientation transform — REQ-PICOO-MEDIA-004/009/017.

use bytes::Bytes;
use thiserror::Error;

use crate::{placeholder::nv12_byte_size, FrameBuffer, FrameBufferPool};

/// An upright NV12 frame produced by [`transform_nv12`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransformedNv12 {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub pixels: Bytes,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum Nv12TransformError {
    #[error("NV12 dimensions must be non-zero and even, got {width}x{height}")]
    InvalidDimensions { width: u32, height: u32 },
    #[error("NV12 stride {stride} is smaller than width {width}")]
    InvalidStride { width: u32, stride: u32 },
    #[error("NV12 buffer has {actual} bytes but requires at least {required}")]
    BufferTooSmall { required: usize, actual: usize },
    #[error("NV12 dimensions overflow the addressable buffer size")]
    SizeOverflow,
}

/// Normalize degrees to `{0, 90, 180, 270}`.
pub fn normalize_rotation_degrees(degrees: u32) -> u32 {
    match degrees % 360 {
        0 => 0,
        r if (45..135).contains(&r) => 90,
        r if (135..225).contains(&r) => 180,
        r if (225..315).contains(&r) => 270,
        _ => 0,
    }
}

/// Rotate clockwise and then optionally mirror in upright output space.
///
/// The no-op path transfers `pixels` without allocation. Every transformed
/// output is compact and allocated exactly once; rotation and mirror are
/// folded into one source-coordinate mapping rather than separate full-frame
/// passes.
pub fn transform_nv12(
    width: u32,
    height: u32,
    stride: u32,
    rotation_degrees: u32,
    mirrored: bool,
    pixels: Bytes,
) -> Result<TransformedNv12, Nv12TransformError> {
    transform_nv12_impl(
        width,
        height,
        stride,
        rotation_degrees,
        mirrored,
        pixels,
        None,
    )
}

/// [`transform_nv12`] using reusable output storage for transformed frames.
/// The no-op path still transfers the Decoder-owned input allocation directly.
pub fn transform_nv12_with_pool(
    width: u32,
    height: u32,
    stride: u32,
    rotation_degrees: u32,
    mirrored: bool,
    pixels: Bytes,
    pool: &FrameBufferPool,
) -> Result<TransformedNv12, Nv12TransformError> {
    transform_nv12_impl(
        width,
        height,
        stride,
        rotation_degrees,
        mirrored,
        pixels,
        Some(pool),
    )
}

fn transform_nv12_impl(
    width: u32,
    height: u32,
    stride: u32,
    rotation_degrees: u32,
    mirrored: bool,
    pixels: Bytes,
    pool: Option<&FrameBufferPool>,
) -> Result<TransformedNv12, Nv12TransformError> {
    let required = validate_layout(width, height, stride, pixels.len())?;
    let pixels = pixels.slice(..required);
    let rotation = normalize_rotation_degrees(rotation_degrees);
    if rotation == 0 && !mirrored {
        return Ok(TransformedNv12 {
            width,
            height,
            stride,
            pixels,
        });
    }

    let (out_width, out_height) = if rotation == 90 || rotation == 270 {
        (height, width)
    } else {
        (width, height)
    };
    let out_stride = out_width;
    let output_size = nv12_byte_size(out_width, out_height);
    let mut output_storage = pool.map_or_else(
        || TransformOutput::Owned(vec![0_u8; output_size]),
        |pool| TransformOutput::Pooled(pool.checkout(output_size)),
    );
    let width = width as usize;
    let height = height as usize;
    let stride = stride as usize;
    let out_width = out_width as usize;
    let out_height = out_height as usize;
    let out_stride = out_stride as usize;
    let source_y_bytes = stride * height;
    let output_y_bytes = out_stride * out_height;

    {
        let output = output_storage.as_mut();
        for output_y in 0..out_height {
            for output_x in 0..out_width {
                let rotated_x = if mirrored {
                    out_width - 1 - output_x
                } else {
                    output_x
                };
                let (source_x, source_y) =
                    map_output_to_source(rotation, rotated_x, output_y, width, height);
                output[output_y * out_stride + output_x] = pixels[source_y * stride + source_x];
            }
        }

        let source_chroma_width = width / 2;
        let source_chroma_height = height / 2;
        let output_chroma_width = out_width / 2;
        let output_chroma_height = out_height / 2;
        for output_y in 0..output_chroma_height {
            for output_x in 0..output_chroma_width {
                // Mirror whole interleaved UV pairs, never individual U/V bytes.
                let rotated_x = if mirrored {
                    output_chroma_width - 1 - output_x
                } else {
                    output_x
                };
                let (source_x, source_y) = map_output_to_source(
                    rotation,
                    rotated_x,
                    output_y,
                    source_chroma_width,
                    source_chroma_height,
                );
                let source = source_y * stride + source_x * 2 + source_y_bytes;
                let destination = output_y * out_stride + output_x * 2 + output_y_bytes;
                output[destination..destination + 2].copy_from_slice(&pixels[source..source + 2]);
            }
        }
    }

    Ok(TransformedNv12 {
        width: out_width as u32,
        height: out_height as u32,
        stride: out_stride as u32,
        pixels: output_storage.freeze(),
    })
}

enum TransformOutput {
    Owned(Vec<u8>),
    Pooled(FrameBuffer),
}

impl AsMut<[u8]> for TransformOutput {
    fn as_mut(&mut self) -> &mut [u8] {
        match self {
            Self::Owned(output) => output,
            Self::Pooled(output) => output.as_mut_slice(),
        }
    }
}

impl TransformOutput {
    fn freeze(self) -> Bytes {
        match self {
            Self::Owned(output) => Bytes::from(output),
            Self::Pooled(output) => output.freeze(),
        }
    }
}

fn validate_layout(
    width: u32,
    height: u32,
    stride: u32,
    actual: usize,
) -> Result<usize, Nv12TransformError> {
    if width == 0 || height == 0 || !width.is_multiple_of(2) || !height.is_multiple_of(2) {
        return Err(Nv12TransformError::InvalidDimensions { width, height });
    }
    if stride < width {
        return Err(Nv12TransformError::InvalidStride { width, stride });
    }
    let required = (stride as usize)
        .checked_mul(height as usize)
        .and_then(|y_bytes| y_bytes.checked_add(y_bytes / 2))
        .ok_or(Nv12TransformError::SizeOverflow)?;
    if actual < required {
        return Err(Nv12TransformError::BufferTooSmall { required, actual });
    }
    Ok(required)
}

fn map_output_to_source(
    rotation: u32,
    output_x: usize,
    output_y: usize,
    source_width: usize,
    source_height: usize,
) -> (usize, usize) {
    match rotation {
        0 => (output_x, output_y),
        90 => (output_y, source_height - 1 - output_x),
        180 => (source_width - 1 - output_x, source_height - 1 - output_y),
        270 => (source_width - 1 - output_y, output_x),
        _ => unreachable!("rotation is normalized before mapping"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::placeholder::nv12_black;

    fn fixture() -> Bytes {
        // Y: 1 2 3 4 / 5 6 7 8; UV: (10,11) (20,21)
        Bytes::from_static(&[1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 20, 21])
    }

    #[test]
    fn no_op_reuses_the_input_allocation() {
        let input = Bytes::from(nv12_black(4, 2));
        let input_pointer = input.as_ptr();
        let output = transform_nv12(4, 2, 4, 0, false, input).expect("valid NV12");
        assert_eq!(output.pixels.as_ptr(), input_pointer);
        assert_eq!((output.width, output.height, output.stride), (4, 2, 4));
    }

    #[test]
    fn mirror_swaps_luma_and_uv_pairs() {
        let output = transform_nv12(4, 2, 4, 0, true, fixture()).expect("mirror");
        assert_eq!(&output.pixels[..8], &[4, 3, 2, 1, 8, 7, 6, 5]);
        assert_eq!(&output.pixels[8..], &[20, 21, 10, 11]);
    }

    #[test]
    fn fused_rotate_then_mirror_writes_final_orientation() {
        let output = transform_nv12(4, 2, 4, 90, true, fixture()).expect("transform");
        assert_eq!((output.width, output.height, output.stride), (2, 4, 2));
        assert_eq!(&output.pixels[..8], &[1, 5, 2, 6, 3, 7, 4, 8]);
        assert_eq!(&output.pixels[8..], &[10, 11, 20, 21]);
    }

    #[test]
    fn fused_transform_mirrors_chroma_pairs_without_swapping_uv() {
        let mut input = vec![0_u8; nv12_byte_size(4, 4)];
        input[16..].copy_from_slice(&[10, 11, 20, 21, 30, 31, 40, 41]);
        let output = transform_nv12(4, 4, 4, 90, true, Bytes::from(input)).expect("transform");
        assert_eq!(&output.pixels[16..], &[10, 11, 30, 31, 20, 21, 40, 41]);
    }

    #[test]
    fn rotation_90_swaps_dimensions_and_moves_corners() {
        let output = transform_nv12(4, 2, 4, 90, false, fixture()).expect("rotate");
        assert_eq!((output.width, output.height, output.stride), (2, 4, 2));
        assert_eq!(&output.pixels[..8], &[5, 1, 6, 2, 7, 3, 8, 4]);
        assert_eq!(&output.pixels[8..], &[10, 11, 20, 21]);
    }

    #[test]
    fn transformed_output_removes_source_padding() {
        let input = Bytes::from_static(&[
            1, 2, 3, 4, 99, 99, 5, 6, 7, 8, 99, 99, 10, 11, 20, 21, 99, 99,
        ]);
        let output = transform_nv12(4, 2, 6, 180, false, input).expect("rotate padded");
        assert_eq!((output.width, output.height, output.stride), (4, 2, 4));
        assert_eq!(output.pixels.len(), 12);
        assert_eq!(&output.pixels[..8], &[8, 7, 6, 5, 4, 3, 2, 1]);
        assert_eq!(&output.pixels[8..], &[20, 21, 10, 11]);
    }

    #[test]
    fn invalid_layout_is_rejected_even_on_no_op_path() {
        assert_eq!(
            transform_nv12(4, 2, 3, 0, false, fixture()),
            Err(Nv12TransformError::InvalidStride {
                width: 4,
                stride: 3
            })
        );
        assert!(matches!(
            transform_nv12(4, 2, 4, 0, false, Bytes::from_static(&[0; 4])),
            Err(Nv12TransformError::BufferTooSmall { .. })
        ));
    }

    #[test]
    fn no_op_slices_extra_tail_without_copying() {
        let input = Bytes::from_static(&[0_u8; 16]);
        let input_pointer = input.as_ptr();
        let output = transform_nv12(4, 2, 4, 0, false, input).expect("valid NV12 prefix");
        assert_eq!(output.pixels.as_ptr(), input_pointer);
        assert_eq!(output.pixels.len(), 12);
    }

    #[test]
    fn normalize_rotation_snaps_to_cardinals() {
        assert_eq!(normalize_rotation_degrees(90), 90);
        assert_eq!(normalize_rotation_degrees(91), 90);
        assert_eq!(normalize_rotation_degrees(180), 180);
        assert_eq!(normalize_rotation_degrees(270), 270);
        assert_eq!(normalize_rotation_degrees(360), 0);
    }

    #[test]
    fn transformed_storage_returns_to_pool_after_all_pixel_views_drop() {
        let pool = FrameBufferPool::with_limits(1, 64);
        let first = transform_nv12_with_pool(4, 2, 4, 90, false, fixture(), &pool)
            .expect("first transform");
        let first_pointer = first.pixels.as_ptr();
        let consumer = first.pixels.clone();
        drop(first);
        assert_eq!(pool.stats().retained_buffers, 0);
        drop(consumer);

        let second = transform_nv12_with_pool(4, 2, 4, 90, false, fixture(), &pool)
            .expect("second transform");
        assert_eq!(second.pixels.as_ptr(), first_pointer);
        assert_eq!(pool.stats().reuses, 1);
    }
}
