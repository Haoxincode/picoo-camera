//! NV12 → RGBA conversion for GPUI preview — REQ-PICOO-UI-004.

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
}
