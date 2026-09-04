//! Horizontal mirror helper — REQ-PICOO-MEDIA-004 (remote output).
//! Clockwise rotation helper — REQ-PICOO-MEDIA-009 (upright pixels for VCam).

use crate::placeholder::nv12_byte_size;

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

/// Clockwise-rotate NV12 pixels so LatestFrameStore / VCam receive upright frames
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::placeholder::nv12_black;

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
}
