//! NV12 → RGBA conversion for GPUI preview — REQ-PICOO-UI-004.
//! Horizontal mirror helper — REQ-PICOO-MEDIA-004 (remote output).
//! Clockwise rotation helper — REQ-PICOO-MEDIA-009 (upright pixels for VCam).

use crate::placeholder::nv12_byte_size;

const PREVIEW_MAX_WIDTH: u32 = 640;

fn clamp_u8(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

/// BT.601 limited-range YUV → RGBA.
fn yuv_to_rgba(y: i32, u: i32, v: i32) -> [u8; 4] {
    let c = (y - 16).max(0);
    let d = u - 128;
    let e = v - 128;
    let r = (298 * c + 409 * e + 128) >> 8;
    let g = (298 * c - 100 * d - 208 * e + 128) >> 8;
    let b = (298 * c + 516 * d + 128) >> 8;
    [clamp_u8(r), clamp_u8(g), clamp_u8(b), 255]
}

/// Horizontally mirror an NV12 frame in place (Y plane + interleaved UV).
///
/// `stride` is the row byte length for both Y and UV planes (typically ≥ `width`).
pub fn nv12_mirror_horizontal(width: u32, height: u32, stride: u32, nv12: &mut [u8]) {
    if width < 2 || height == 0 || stride < width {
        return;
    }
    let w = width as usize;
    let h = height as usize;
    let s = stride as usize;
    let y_plane = s.saturating_mul(h);
    let uv_needed = s.saturating_mul(h / 2);
    if nv12.len() < y_plane + uv_needed {
        return;
    }

    for row in 0..h {
        let base = row * s;
        for x in 0..(w / 2) {
            nv12.swap(base + x, base + (w - 1 - x));
        }
    }

    let uv_rows = h / 2;
    let pairs = w / 2;
    for row in 0..uv_rows {
        let base = y_plane + row * s;
        for p in 0..(pairs / 2) {
            let left = base + p * 2;
            let right = base + (pairs - 1 - p) * 2;
            nv12.swap(left, right);
            nv12.swap(left + 1, right + 1);
        }
    }
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

/// Clockwise-rotate NV12 pixels so FrameHub / VCam receive upright frames
/// (REQ-PICOO-MEDIA-009). Returns `(out_w, out_h, out_stride, pixels)`.
///
/// `rotation_degrees` is snapped to 0/90/180/270. For 0°, returns `None` so the
/// caller can keep the original buffer without copying.
pub fn nv12_rotate_clockwise(
    width: u32,
    height: u32,
    stride: u32,
    rotation_degrees: u32,
    nv12: &[u8],
) -> Option<(u32, u32, u32, Vec<u8>)> {
    let rot = normalize_rotation_degrees(rotation_degrees);
    if rot == 0 {
        return None;
    }
    if width < 2
        || height < 2
        || !width.is_multiple_of(2)
        || !height.is_multiple_of(2)
        || stride < width
    {
        return None;
    }
    let expected = (stride as usize) * (height as usize) * 3 / 2;
    if nv12.len() < expected {
        return None;
    }

    let (out_w, out_h) = if rot == 180 {
        (width, height)
    } else {
        (height, width)
    };
    let out_stride = out_w;
    let mut out = vec![0u8; nv12_byte_size(out_w, out_h)];
    let w = width as usize;
    let h = height as usize;
    let s = stride as usize;
    let ow = out_w as usize;
    let oh = out_h as usize;
    let os = out_stride as usize;
    let src_y_plane = s * h;
    let dst_y_plane = os * oh;

    for y in 0..oh {
        for x in 0..ow {
            let (sx, sy) = match rot {
                // out is (height × width); 90° CW: column y becomes row.
                90 => (y, h - 1 - x),
                180 => (w - 1 - x, h - 1 - y),
                270 => (w - 1 - y, x),
                _ => unreachable!(),
            };
            out[y * os + x] = nv12[sy * s + sx];
        }
    }

    // Chroma: sample UV at the 2×2 cell covering the rotated luma coordinate.
    for y in (0..oh).step_by(2) {
        for x in (0..ow).step_by(2) {
            let (sx, sy) = match rot {
                90 => (y, h - 1 - x),
                180 => (w - 1 - x, h - 1 - y),
                270 => (w - 1 - y, x),
                _ => unreachable!(),
            };
            let src_uv = src_y_plane + (sy / 2) * s + (sx / 2) * 2;
            let dst_uv = dst_y_plane + (y / 2) * os + (x / 2) * 2;
            out[dst_uv] = nv12[src_uv];
            out[dst_uv + 1] = nv12[src_uv + 1];
        }
    }

    Some((out_w, out_h, out_stride, out))
}

/// Center-crop an upright NV12 frame to the target aspect ratio and scale it to
/// the target dimensions (REQ-PICOO-MEDIA-013). The crop prefers the largest
/// exact even-sized rectangle; formats such as 854×480 fall back to the nearest
/// chroma-aligned ratio because that geometry cannot be represented exactly.
///
/// Returns `None` when the source already matches the requested geometry or an
/// valid chroma-aligned crop is impossible.
pub fn nv12_center_crop_scale(
    width: u32,
    height: u32,
    stride: u32,
    target_width: u32,
    target_height: u32,
    nv12: &[u8],
) -> Option<(u32, u32, u32, Vec<u8>)> {
    if width < 2
        || height < 2
        || target_width < 2
        || target_height < 2
        || !width.is_multiple_of(2)
        || !height.is_multiple_of(2)
        || !target_width.is_multiple_of(2)
        || !target_height.is_multiple_of(2)
        || stride < width
    {
        return None;
    }
    let expected = (stride as usize) * (height as usize) * 3 / 2;
    if nv12.len() < expected {
        return None;
    }
    if width == target_width && height == target_height && stride == width {
        return None;
    }

    let aspect_gcd = gcd(target_width, target_height);
    let aspect_width = target_width / aspect_gcd;
    let aspect_height = target_height / aspect_gcd;
    let max_scale = (width / aspect_width).min(height / aspect_height);
    let even_scale = max_scale - (max_scale % 2);
    let (crop_width, crop_height) = if even_scale > 0 {
        (aspect_width * even_scale, aspect_height * even_scale)
    } else if width as u64 * target_height as u64 > height as u64 * target_width as u64 {
        (
            nearest_even_ratio(
                height as u64 * target_width as u64,
                target_height as u64,
                width,
            ),
            height,
        )
    } else {
        (
            width,
            nearest_even_ratio(
                width as u64 * target_height as u64,
                target_width as u64,
                height,
            ),
        )
    };
    if crop_width < 2 || crop_height < 2 {
        return None;
    }
    let crop_x = ((width - crop_width) / 2) & !1;
    let crop_y = ((height - crop_height) / 2) & !1;

    let out_stride = target_width;
    let mut out = vec![0u8; nv12_byte_size(target_width, target_height)];
    let src_stride = stride as usize;
    let dst_stride = out_stride as usize;
    let src_y_plane = src_stride * height as usize;
    let dst_y_plane = dst_stride * target_height as usize;

    let x_map: Vec<usize> = (0..target_width)
        .map(|x| (crop_x + x * crop_width / target_width) as usize)
        .collect();
    let y_map: Vec<usize> = (0..target_height)
        .map(|y| (crop_y + y * crop_height / target_height) as usize)
        .collect();
    for (dst_y, &src_y) in y_map.iter().enumerate() {
        let src_row = src_y * src_stride;
        let dst_row = dst_y * dst_stride;
        for (dst_x, &src_x) in x_map.iter().enumerate() {
            out[dst_row + dst_x] = nv12[src_row + src_x];
        }
    }

    let target_uv_width = target_width / 2;
    let target_uv_height = target_height / 2;
    let crop_uv_width = crop_width / 2;
    let crop_uv_height = crop_height / 2;
    let crop_uv_x = crop_x / 2;
    let crop_uv_y = crop_y / 2;
    for dst_y in 0..target_uv_height {
        let src_y = crop_uv_y + dst_y * crop_uv_height / target_uv_height;
        let src_row = src_y_plane + src_y as usize * src_stride;
        let dst_row = dst_y_plane + dst_y as usize * dst_stride;
        for dst_x in 0..target_uv_width {
            let src_x = crop_uv_x + dst_x * crop_uv_width / target_uv_width;
            let src = src_row + src_x as usize * 2;
            let dst = dst_row + dst_x as usize * 2;
            out[dst] = nv12[src];
            out[dst + 1] = nv12[src + 1];
        }
    }

    Some((target_width, target_height, out_stride, out))
}

fn gcd(mut left: u32, mut right: u32) -> u32 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left
}

fn nearest_even_ratio(numerator: u64, denominator: u64, limit: u32) -> u32 {
    let rounded = ((numerator + denominator / 2) / denominator).min(limit as u64) as u32;
    if rounded.is_multiple_of(2) {
        return rounded;
    }
    let lower = rounded.saturating_sub(1);
    let upper = rounded.saturating_add(1).min(limit);
    let lower_error = numerator.abs_diff(lower as u64 * denominator);
    let upper_error = numerator.abs_diff(upper as u64 * denominator);
    if upper.is_multiple_of(2) && upper_error < lower_error {
        upper
    } else {
        lower
    }
}

/// Downscaled RGBA preview suitable for GPUI `RenderImage` (max width 640 by default).
pub fn nv12_preview_rgba(
    width: u32,
    height: u32,
    stride: u32,
    nv12: &[u8],
) -> Option<(u32, u32, Vec<u8>)> {
    nv12_preview_rgba_max_width(width, height, stride, nv12, PREVIEW_MAX_WIDTH)
}

pub fn nv12_preview_rgba_max_width(
    width: u32,
    height: u32,
    stride: u32,
    nv12: &[u8],
    max_width: u32,
) -> Option<(u32, u32, Vec<u8>)> {
    if width == 0 || height == 0 || stride == 0 {
        return None;
    }
    let expected = nv12_byte_size(width, height);
    if nv12.len() < expected {
        return None;
    }

    let scale = if width > max_width {
        max_width as f32 / width as f32
    } else {
        1.0
    };
    let out_w = ((width as f32 * scale).round() as u32).max(1);
    let out_h = ((height as f32 * scale).round() as u32).max(1);
    let y_stride = stride as usize;
    let uv_offset = y_stride * height as usize;
    let mut rgba = vec![0u8; (out_w * out_h * 4) as usize];

    for oy in 0..out_h {
        let sy = ((oy as f32 / out_h as f32) * height as f32) as u32;
        let sy = sy.min(height - 1);
        let uv_row = uv_offset + (sy as usize / 2) * y_stride;
        for ox in 0..out_w {
            let sx = ((ox as f32 / out_w as f32) * width as f32) as u32;
            let sx = sx.min(width - 1);
            let y_idx = sy as usize * y_stride + sx as usize;
            let uv_idx = uv_row + (sx as usize / 2) * 2;
            let y = nv12[y_idx] as i32;
            let u = nv12.get(uv_idx).copied().unwrap_or(128) as i32;
            let v = nv12.get(uv_idx + 1).copied().unwrap_or(128) as i32;
            let px = yuv_to_rgba(y, u, v);
            let dst = ((oy * out_w + ox) * 4) as usize;
            rgba[dst..dst + 4].copy_from_slice(&px);
        }
    }

    Some((out_w, out_h, rgba))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::placeholder::{nv12_black, PLACEHOLDER_HEIGHT, PLACEHOLDER_WIDTH};

    #[test]
    fn preview_from_placeholder_nv12() {
        let nv12 = nv12_black(PLACEHOLDER_WIDTH, PLACEHOLDER_HEIGHT);
        let (w, h, rgba) = nv12_preview_rgba(
            PLACEHOLDER_WIDTH,
            PLACEHOLDER_HEIGHT,
            PLACEHOLDER_WIDTH,
            &nv12,
        )
        .expect("preview");
        assert!(w <= PREVIEW_MAX_WIDTH);
        assert!(h > 0);
        assert_eq!(rgba.len(), (w * h * 4) as usize);
        // Black frame → near-zero RGB (alpha is 255)
        assert!(rgba[0] <= 16 && rgba[1] <= 16 && rgba[2] <= 16);
        assert_eq!(rgba[3], 255);
    }

    #[test]
    fn mirror_swaps_left_and_right_y_samples() {
        // REQ-PICOO-MEDIA-004
        let width = 4u32;
        let height = 2u32;
        let stride = 4u32;
        let mut nv12 = vec![0u8; nv12_byte_size(width, height)];
        // Y row0: 1 2 3 4 → after mirror 4 3 2 1
        nv12[0] = 1;
        nv12[1] = 2;
        nv12[2] = 3;
        nv12[3] = 4;
        nv12_mirror_horizontal(width, height, stride, &mut nv12);
        assert_eq!(&nv12[0..4], &[4, 3, 2, 1]);
    }

    #[test]
    fn rotate_90_swaps_dims_and_moves_y_corner() {
        // REQ-PICOO-MEDIA-009
        let width = 4u32;
        let height = 2u32;
        let stride = 4u32;
        let mut nv12 = vec![128u8; nv12_byte_size(width, height)];
        // Y:
        // 1 2 3 4
        // 5 6 7 8
        for (i, v) in [1u8, 2, 3, 4, 5, 6, 7, 8].into_iter().enumerate() {
            nv12[i] = v;
        }
        let (ow, oh, os, out) =
            nv12_rotate_clockwise(width, height, stride, 90, &nv12).expect("rotate");
        assert_eq!((ow, oh, os), (2, 4, 2));
        // 90° CW of
        // 1 2 3 4
        // 5 6 7 8
        // →
        // 5 1
        // 6 2
        // 7 3
        // 8 4
        assert_eq!(out[0], 5);
        assert_eq!(out[1], 1);
        assert_eq!(out[2], 6);
        assert_eq!(out[3], 2);
        assert_eq!(out[6], 8);
        assert_eq!(out[7], 4);
    }

    #[test]
    fn rotate_0_returns_none() {
        let nv12 = nv12_black(4, 2);
        assert!(nv12_rotate_clockwise(4, 2, 4, 0, &nv12).is_none());
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
    fn portrait_frame_is_center_cropped_and_scaled_to_landscape() {
        // 36×64 portrait input can hold an exact 32×18 center crop.
        let width = 36u32;
        let height = 64u32;
        let mut nv12 = vec![128u8; nv12_byte_size(width, height)];
        for y in 0..height {
            let row = y as usize * width as usize;
            nv12[row..row + width as usize].fill(y as u8);
        }

        let (out_width, out_height, out_stride, out) =
            nv12_center_crop_scale(width, height, width, 64, 36, &nv12).expect("center crop");

        assert_eq!((out_width, out_height, out_stride), (64, 36, 64));
        assert_eq!(out.len(), nv12_byte_size(64, 36));
        // Center crop is rows 22..39 (the exact center is chroma-aligned).
        assert_eq!(out[0], 22);
        assert_eq!(out[(35 * 64) as usize], 39);
    }

    #[test]
    fn matching_geometry_avoids_an_extra_copy() {
        let nv12 = nv12_black(64, 36);
        assert!(nv12_center_crop_scale(64, 36, 64, 64, 36, &nv12).is_none());
    }

    #[test]
    fn portrait_480p_uses_nearest_chroma_aligned_crop() {
        let width = 480;
        let height = 854;
        let nv12 = nv12_black(width, height);
        let (out_width, out_height, out_stride, out) =
            nv12_center_crop_scale(width, height, width, 854, 480, &nv12)
                .expect("480p center crop");
        assert_eq!((out_width, out_height, out_stride), (854, 480, 854));
        assert_eq!(out.len(), nv12_byte_size(854, 480));
    }
}
