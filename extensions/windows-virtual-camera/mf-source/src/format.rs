//! Media-type policy shared by host tests and the Windows COM implementation.

pub const DEFAULT_WIDTH: u32 = 1280;
pub const DEFAULT_HEIGHT: u32 = 720;
pub const FRAME_RATE_NUM: u32 = 30;
pub const FRAME_RATE_DEN: u32 = 1;
pub const SAMPLE_DURATION_100NS: i64 = 333_333;

pub fn is_supported_output_size(width: u32, height: u32) -> bool {
    matches!((width, height), (854, 480) | (1280, 720) | (1920, 1080))
}

pub fn nv12_len(width: u32, height: u32) -> Option<usize> {
    if width == 0 || height == 0 || !width.is_multiple_of(2) || !height.is_multiple_of(2) {
        return None;
    }
    (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(3)?
        .checked_div(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_ladder_matches_sender_abr() {
        assert!(is_supported_output_size(854, 480));
        assert!(is_supported_output_size(1280, 720));
        assert!(is_supported_output_size(1920, 1080));
        assert!(!is_supported_output_size(640, 480));
        assert!(!is_supported_output_size(3840, 2160));
    }

    #[test]
    fn nv12_size_is_checked() {
        assert_eq!(nv12_len(1280, 720), Some(1_382_400));
        assert_eq!(nv12_len(1920, 1080), Some(3_110_400));
        assert_eq!(nv12_len(0, 720), None);
        assert_eq!(nv12_len(1279, 720), None);
    }
}
