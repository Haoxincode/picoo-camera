//! Explicit source request — REQ-PICOO-MEDIA-043. Native records arrive later.
use picoo_bitstream::Codec;

/// The product's progressive 8-bit 4:2:0 SDR source combinations.
/// Native offers must prove support; constructing this value does not prove it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceFormat {
    pub codec: Codec,
    pub height: u32,
    pub fps: u32,
}

impl SourceFormat {
    pub fn is_product_format(self) -> bool {
        matches!(self.height, 720 | 1080) && matches!(self.fps, 30 | 60)
    }

    pub fn matches(self, config: &crate::StreamConfigParams) -> bool {
        self.is_product_format()
            && self.codec == config.configuration.codec()
            && self.height == config.height
            && config.width == if self.height == 720 { 1280 } else { 1920 }
            && self.fps == config.fps
    }
}
