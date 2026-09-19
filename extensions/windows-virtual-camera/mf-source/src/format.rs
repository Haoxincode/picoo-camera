//! Media-type policy shared by host tests and the Windows COM implementation.

pub const DEFAULT_WIDTH: u32 = 1280;
pub const DEFAULT_HEIGHT: u32 = 720;
pub const DEFAULT_FRAME_RATE_NUM: u32 = 30;
pub const DEFAULT_FRAME_RATE_DEN: u32 = 1;
pub const FRAME_RATES: [(u32, u32); 2] = [(30, 1), (60, 1)];

/// Kept for callers that need the default media type's duration. Variable
/// frame-rate media types use `sample_duration_100ns` instead.
pub const SAMPLE_DURATION_100NS: i64 = 333_333;

pub const fn is_supported_frame_rate(num: u32, den: u32) -> bool {
    matches!((num, den), (30, 1) | (60, 1))
}

pub fn sample_duration_100ns(num: u32, den: u32) -> Option<i64> {
    if !is_supported_frame_rate(num, den) {
        return None;
    }
    i64::try_from(
        (10_000_000_u64)
            .checked_mul(u64::from(den))?
            .checked_div(u64::from(num))?,
    )
    .ok()
}

pub fn is_supported_output_size(width: u32, height: u32) -> bool {
    matches!((width, height), (1280, 720) | (1920, 1080))
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
    fn supports_only_the_formal_vcam_rates() {
        assert!(is_supported_frame_rate(30, 1));
        assert!(is_supported_frame_rate(60, 1));
        assert!(!is_supported_frame_rate(29_970, 1_000));
        assert!(!is_supported_frame_rate(30, 0));
        assert_eq!(sample_duration_100ns(30, 1), Some(333_333));
        assert_eq!(sample_duration_100ns(60, 1), Some(166_666));
        assert_eq!(sample_duration_100ns(24, 1), None);
    }

    #[test]
    fn output_ladder_matches_sender_abr() {
        assert!(is_supported_output_size(1280, 720));
        assert!(is_supported_output_size(1920, 1080));
        assert!(!is_supported_output_size(854, 480));
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
