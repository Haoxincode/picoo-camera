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

    pub(crate) fn is_offered_by(self, caps: &picoo_protocol::control::Capabilities) -> bool {
        use picoo_protocol::control::{ColorRange, FrameRate, Resolution, VideoCodec, VideoFormat};
        if !self.is_product_format() || caps.validate().is_err() {
            return false;
        }
        let format = VideoFormat::sdr_709(
            match self.codec {
                Codec::Avc => VideoCodec::Avc,
                Codec::Hevc => VideoCodec::Hevc,
            },
            Resolution {
                width: if self.height == 720 { 1280 } else { 1920 },
                height: self.height,
            },
            FrameRate {
                numerator: self.fps,
                denominator: 1,
            },
            ColorRange::Limited,
        );
        // A request names the visible camera format. Storage padding and crop
        // origin are native facts; final Capabilities::supports still requires
        // their exact record-derived values, without combining different offers.
        caps.offers.iter().any(|offer| {
            let Some(candidate) = offer.format else {
                return false;
            };
            let Some(crop) = candidate.visible_rect else {
                return false;
            };
            let requested = VideoFormat {
                coded_size: candidate.coded_size,
                visible_rect: Some(picoo_protocol::control::VisibleRect {
                    width: format.visible_rect.unwrap().width,
                    height: self.height,
                    ..crop
                }),
                ..format
            };
            candidate == requested
        })
    }

    pub fn matches(self, config: &crate::StreamConfigParams) -> bool {
        self.is_product_format()
            && self.codec == config.configuration.codec()
            && self.height == config.height
            && config.width == if self.height == 720 { 1280 } else { 1920 }
            && self.fps == config.fps
    }
}
