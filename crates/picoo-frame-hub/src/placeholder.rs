//! Placeholder NV12 frames — REQ-PICOO-FRAME-004 / FRAME-005.
//!
//! Black background + brand mark + status text drawn into the Y plane.

pub const PLACEHOLDER_WIDTH: u32 = 1280;
pub const PLACEHOLDER_HEIGHT: u32 = 720;

/// NV12 black frame (Y=0, UV=128).
pub fn nv12_black(width: u32, height: u32) -> Vec<u8> {
    let y_size = (width as usize) * (height as usize);
    let uv_size = y_size / 2;
    let mut buf = vec![0u8; y_size + uv_size];
    buf[y_size..].fill(128);
    buf
}

/// Waiting-for-phone placeholder used when no sender is connected.
pub fn waiting_placeholder() -> Vec<u8> {
    branded_status_placeholder(b"Waiting for phone...")
}

/// Reconnect placeholder after last-frame hold (REQ-PICOO-FRAME-005 / FR-VCAM-004).
pub fn reconnecting_placeholder() -> Vec<u8> {
    branded_status_placeholder(b"Reconnecting...")
}

fn branded_status_placeholder(status: &[u8]) -> Vec<u8> {
    let mut buf = nv12_black(PLACEHOLDER_WIDTH, PLACEHOLDER_HEIGHT);
    draw_branded_text(&mut buf, PLACEHOLDER_WIDTH, PLACEHOLDER_HEIGHT, status);
    buf
}

pub fn nv12_byte_size(width: u32, height: u32) -> usize {
    (width as usize) * (height as usize) * 3 / 2
}

/// Minimal 5×7 glyphs for ASCII brand / status text (REQ-PICOO-FRAME-004).
fn glyph(c: u8) -> [u8; 7] {
    match c {
        b' ' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        b'.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C],
        b'A' | b'a' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        b'C' | b'c' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        b'e' => [0x00, 0x00, 0x0E, 0x11, 0x1F, 0x10, 0x0E],
        b'f' => [0x06, 0x09, 0x08, 0x1E, 0x08, 0x08, 0x08],
        b'g' => [0x00, 0x00, 0x0F, 0x11, 0x0F, 0x01, 0x0E],
        b'h' => [0x10, 0x10, 0x1E, 0x11, 0x11, 0x11, 0x11],
        b'i' => [0x04, 0x00, 0x0C, 0x04, 0x04, 0x04, 0x0E],
        b'm' => [0x00, 0x00, 0x1B, 0x15, 0x15, 0x15, 0x15],
        b'n' => [0x00, 0x00, 0x1E, 0x11, 0x11, 0x11, 0x11],
        b'o' => [0x00, 0x00, 0x0E, 0x11, 0x11, 0x11, 0x0E],
        b'P' | b'p' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        b'R' | b'r' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        b't' => [0x08, 0x08, 0x1E, 0x08, 0x08, 0x09, 0x06],
        b'W' | b'w' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x1B, 0x11],
        b'y' => [0x00, 0x00, 0x11, 0x11, 0x0F, 0x01, 0x0E],
        _ => [0x1F, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1F],
    }
}

fn draw_branded_text(nv12: &mut [u8], width: u32, height: u32, status: &[u8]) {
    let scale = if width >= 1280 { 4u32 } else { 2u32 };
    let brand = b"Picoo Camera";

    let brand_w = text_pixel_width(brand, scale);
    let status_w = text_pixel_width(status, scale);
    let brand_x = width.saturating_sub(brand_w) / 2;
    let status_x = width.saturating_sub(status_w) / 2;
    let brand_y = height / 2 - 10 * scale;
    let status_y = height / 2 + 4 * scale;

    blit_text(nv12, width, brand, brand_x, brand_y, scale, 220);
    blit_text(nv12, width, status, status_x, status_y, scale, 180);
}

fn text_pixel_width(text: &[u8], scale: u32) -> u32 {
    let glyph_w = 6u32 * scale;
    text.len() as u32 * glyph_w
}

fn blit_text(
    nv12: &mut [u8],
    width: u32,
    text: &[u8],
    origin_x: u32,
    origin_y: u32,
    scale: u32,
    luma: u8,
) {
    let mut cursor_x = origin_x;
    for &ch in text {
        blit_glyph(nv12, width, ch, cursor_x, origin_y, scale, luma);
        cursor_x = cursor_x.saturating_add(6 * scale);
    }
}

fn blit_glyph(
    nv12: &mut [u8],
    width: u32,
    ch: u8,
    origin_x: u32,
    origin_y: u32,
    scale: u32,
    luma: u8,
) {
    let bits = glyph(ch);
    for (row, row_bits) in bits.iter().enumerate() {
        for col in 0..5u32 {
            if (row_bits >> (4 - col)) & 1 == 0 {
                continue;
            }
            for dy in 0..scale {
                for dx in 0..scale {
                    let x = origin_x + col * scale + dx;
                    let y = origin_y + row as u32 * scale + dy;
                    if x >= width {
                        continue;
                    }
                    let idx = (y as usize) * (width as usize) + (x as usize);
                    if idx < nv12.len() {
                        nv12[idx] = luma;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waiting_placeholder_is_valid_nv12_size() {
        let frame = waiting_placeholder();
        assert_eq!(
            frame.len(),
            nv12_byte_size(PLACEHOLDER_WIDTH, PLACEHOLDER_HEIGHT)
        );
    }

    #[test]
    fn waiting_placeholder_is_not_pure_black() {
        let frame = waiting_placeholder();
        let y_plane = &frame[..PLACEHOLDER_WIDTH as usize * PLACEHOLDER_HEIGHT as usize];
        assert!(
            y_plane.iter().any(|&y| y > 0),
            "brand/status text must light some Y pixels"
        );
        let uv = &frame[PLACEHOLDER_WIDTH as usize * PLACEHOLDER_HEIGHT as usize..];
        assert!(uv.iter().all(|&u| u == 128));
    }

    #[test]
    fn reconnecting_placeholder_differs_from_waiting() {
        let wait = waiting_placeholder();
        let recon = reconnecting_placeholder();
        assert_eq!(wait.len(), recon.len());
        assert_ne!(
            wait, recon,
            "reconnect copy must differ from idle waiting text"
        );
        let y = &recon[..PLACEHOLDER_WIDTH as usize * PLACEHOLDER_HEIGHT as usize];
        assert!(y.iter().any(|&v| v > 0));
    }
}
