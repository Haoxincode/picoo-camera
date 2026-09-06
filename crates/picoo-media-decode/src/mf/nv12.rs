//! Explicit native allocation and SPS crop; never infer layout from byte length.
//! REQ-PICOO-MEDIA-032.

use crate::DecodeError;
use picoo_bitstream::AvcSpsFacts;

pub(crate) fn copy_visible_nv12(
    source: &[u8],
    row_zero: usize,
    stride: usize,
    facts: &AvcSpsFacts,
) -> Result<Vec<u8>, DecodeError> {
    let invalid = || DecodeError::Platform("invalid native NV12 allocation/crop".into());
    let (coded_width, coded_height) = (facts.coded_width as usize, facts.coded_height as usize);
    let (x, y) = (facts.visible_x as usize, facts.visible_y as usize);
    let (width, height) = (facts.visible_width as usize, facts.visible_height as usize);
    if [coded_width, coded_height, width, height].contains(&0)
        || [coded_width, coded_height, x, y, width, height]
            .iter()
            .any(|v| v % 2 != 0)
        || stride < coded_width
        || x.checked_add(width).is_none_or(|end| end > coded_width)
        || y.checked_add(height).is_none_or(|end| end > coded_height)
    {
        return Err(invalid());
    }
    let uv_base = stride
        .checked_mul(coded_height)
        .and_then(|n| row_zero.checked_add(n))
        .ok_or_else(invalid)?;
    let output_len = width
        .checked_mul(height)
        .and_then(|n| n.checked_mul(3))
        .map(|n| n / 2)
        .ok_or_else(invalid)?;
    // Check the entire declared allocation before allocating or reading rows.
    let allocation_end = stride
        .checked_mul(coded_height / 2)
        .and_then(|n| uv_base.checked_add(n))
        .ok_or_else(invalid)?;
    if allocation_end > source.len() {
        return Err(invalid());
    }
    let mut output = vec![0; output_len];
    for row in 0..height {
        let start = row_zero + (y + row) * stride + x;
        output[row * width..(row + 1) * width].copy_from_slice(&source[start..start + width]);
    }
    let output_uv = width * height;
    for row in 0..height / 2 {
        let start = uv_base + (y / 2 + row) * stride + x;
        let target = output_uv + row * width;
        output[target..target + width].copy_from_slice(&source[start..start + width]);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(
        width: u32,
        height: u32,
        x: u32,
        y: u32,
        visible_width: u32,
        visible_height: u32,
    ) -> AvcSpsFacts {
        AvcSpsFacts {
            coded_width: width,
            coded_height: height,
            visible_x: x,
            visible_y: y,
            visible_width,
            visible_height,
            pixel_aspect_ratio: None,
            color: None,
            chroma_location: 0,
        }
    }

    #[test]
    fn copies_offset_crop_from_pitched_allocation_without_reading_padding() {
        let facts = facts(8, 6, 2, 2, 4, 2);
        let stride = 12;
        let row_zero = 3;
        let mut source = vec![255; row_zero + stride * 9];
        source[row_zero + 2 * stride + 2..row_zero + 2 * stride + 6].copy_from_slice(&[1, 2, 3, 4]);
        source[row_zero + 3 * stride + 2..row_zero + 3 * stride + 6].copy_from_slice(&[5, 6, 7, 8]);
        source[row_zero + 7 * stride + 2..row_zero + 7 * stride + 6]
            .copy_from_slice(&[21, 31, 22, 32]);
        assert_eq!(
            copy_visible_nv12(&source, row_zero, stride, &facts).unwrap(),
            [1, 2, 3, 4, 5, 6, 7, 8, 21, 31, 22, 32]
        );
    }

    #[test]
    fn coded_height_sets_uv_origin_for_1080p() {
        let facts = facts(1920, 1088, 0, 0, 1920, 1080);
        let mut source = vec![0; 1920 * 1088 * 3 / 2];
        source[1920 * 1088..1920 * 1088 + 2].copy_from_slice(&[23, 211]);
        let output = copy_visible_nv12(&source, 0, 1920, &facts).unwrap();
        assert_eq!(&output[1920 * 1080..1920 * 1080 + 2], &[23, 211]);
    }

    #[test]
    fn native_fixture_has_distinct_coded_and_visible_geometry() {
        let (sps, _) =
            picoo_bitstream::avc::extract_sps_pps(picoo_testkit::AVC_64X64_BT709_IDR).unwrap();
        let facts = AvcSpsFacts::parse(&sps).unwrap();
        assert_eq!((facts.coded_width, facts.coded_height), (192, 96));
        assert_eq!((facts.visible_width, facts.visible_height), (64, 64));
        let source = vec![17; 192 * 96 * 3 / 2];
        assert_eq!(
            copy_visible_nv12(&source, 0, 192, &facts).unwrap(),
            vec![17; 64 * 64 * 3 / 2]
        );
    }

    #[test]
    fn invalid_bounds_and_layout_are_rejected() {
        let valid = facts(8, 6, 0, 0, 8, 6);
        assert!(copy_visible_nv12(&[0; 71], 0, 8, &valid).is_err());
        assert!(copy_visible_nv12(&[0; 72], 0, 7, &valid).is_err());
        assert!(copy_visible_nv12(&[0; 72], usize::MAX, 8, &valid).is_err());
        assert!(copy_visible_nv12(&[0; 72], 0, usize::MAX, &valid).is_err());
        for crop in [
            facts(8, 6, 1, 0, 4, 2),
            facts(8, 6, 6, 0, 4, 2),
            facts(8, 6, 0, 4, 4, 4),
        ] {
            assert!(copy_visible_nv12(&[0; 72], 0, 8, &crop).is_err());
        }
    }
}
