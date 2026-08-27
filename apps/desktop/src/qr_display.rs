//! QR rendering for PUC-003 Show QR Code (CLI ASCII + GPUI bitmap).

use qrcode::{Color, QrCode};

pub fn render_qr_ascii(payload: &str) -> Result<String, String> {
    let code = QrCode::new(payload.as_bytes()).map_err(|e| e.to_string())?;
    Ok(code
        .render::<char>()
        .quiet_zone(true)
        .module_dimensions(1, 1)
        .build())
}

/// RGBA8 bitmap suitable for GPUI `RenderImage` (REQ-PICOO-DISCOVERY-003 / PUC-003).
///
/// `module_px` is the pixel size of one QR module (including quiet zone modules).
pub fn render_qr_rgba(payload: &str, module_px: u32) -> Result<(u32, u32, Vec<u8>), String> {
    if module_px == 0 {
        return Err("module_px must be > 0".into());
    }
    let code = QrCode::new(payload.as_bytes()).map_err(|e| e.to_string())?;
    let module_count = code.width();
    let quiet = 4usize;
    let side_modules = module_count + quiet * 2;
    let side_px = (side_modules as u32)
        .checked_mul(module_px)
        .ok_or_else(|| "QR bitmap too large".to_string())?;
    let colors = code.to_colors();
    let mut rgba = vec![255u8; (side_px as usize) * (side_px as usize) * 4];

    for my in 0..module_count {
        for mx in 0..module_count {
            let dark = colors[my * module_count + mx] == Color::Dark;
            if !dark {
                continue;
            }
            let x0 = ((mx + quiet) as u32) * module_px;
            let y0 = ((my + quiet) as u32) * module_px;
            for py in 0..module_px {
                for px in 0..module_px {
                    let idx = (((y0 + py) * side_px + (x0 + px)) as usize) * 4;
                    rgba[idx] = 0;
                    rgba[idx + 1] = 0;
                    rgba[idx + 2] = 0;
                    rgba[idx + 3] = 255;
                }
            }
        }
    }

    Ok((side_px, side_px, rgba))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_non_empty_ascii_qr() {
        let art = render_qr_ascii("{\"v\":1}").expect("qr");
        assert!(art.contains('█') || art.contains('#'));
        assert!(art.lines().count() > 5);
    }

    #[test]
    fn renders_rgba_bitmap_with_dark_modules() {
        let (w, h, rgba) = render_qr_rgba("{\"v\":1,\"host\":\"1.2.3.4\"}", 4).expect("rgba");
        assert_eq!(w, h);
        assert!(w >= 64);
        assert_eq!(rgba.len(), (w as usize) * (h as usize) * 4);
        let dark_pixels = rgba
            .chunks_exact(4)
            .filter(|p| p[0] == 0 && p[3] == 255)
            .count();
        let light_pixels = rgba
            .chunks_exact(4)
            .filter(|p| p[0] == 255 && p[3] == 255)
            .count();
        assert!(dark_pixels > 100);
        assert!(light_pixels > dark_pixels);
    }
}
