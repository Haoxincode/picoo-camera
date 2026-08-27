//! Placeholder NV12 frames — REQ-PICOO-FRAME-004.

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
    nv12_black(PLACEHOLDER_WIDTH, PLACEHOLDER_HEIGHT)
}

pub fn nv12_byte_size(width: u32, height: u32) -> usize {
    (width as usize) * (height as usize) * 3 / 2
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
}
