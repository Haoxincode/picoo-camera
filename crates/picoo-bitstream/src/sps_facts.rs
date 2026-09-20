//! Codec-independent source presentation facts; native output is validated separately.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoColorFacts {
    pub full_range: bool,
    pub primaries: u8,
    pub transfer: u8,
    pub matrix: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoSpsFacts {
    pub coded_width: u32,
    pub coded_height: u32,
    pub visible_x: u32,
    pub visible_y: u32,
    pub visible_width: u32,
    pub visible_height: u32,
    /// None means unspecified, never implicitly square.
    pub pixel_aspect_ratio: Option<(u32, u32)>,
    /// Unspecified CICP values remain unspecified (2).
    pub color: Option<VideoColorFacts>,
    /// AVC/HEVC infer type 0 when chroma_loc_info_present_flag is absent.
    pub chroma_location: u8,
}
