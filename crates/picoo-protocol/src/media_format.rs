//! Complete decoder offers and bounded admission — REQ-PICOO-PROTOCOL-015.

use crate::control::{
    Capabilities, ColorDescription, ColorMatrix, ColorPrimaries, ColorRange, ColorTransfer,
    FrameRate, Resolution, VideoChroma, VideoCodec, VideoFormat, VideoProfile, VisibleRect,
};

pub const MAX_DECODER_OFFERS: usize = 16;
pub const MAX_MEDIA_ACCESS_UNIT_BYTES: u32 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("invalid media format: {0}")]
pub struct MediaFormatError(pub &'static str);

impl VideoFormat {
    /// Describe an explicit SDR format. This constructs a value, not capability
    /// evidence: native adapters must still prove support before advertising it.
    pub fn sdr_709(
        codec: VideoCodec,
        coded_size: Resolution,
        frame_rate: FrameRate,
        range: ColorRange,
    ) -> Self {
        Self {
            codec: codec as i32,
            profile: match codec {
                VideoCodec::Avc => VideoProfile::AvcHigh,
                VideoCodec::Hevc => VideoProfile::HevcMain,
                VideoCodec::Unspecified => VideoProfile::Unspecified,
            } as i32,
            bit_depth: 8,
            chroma: VideoChroma::Yuv420 as i32,
            visible_rect: Some(VisibleRect {
                x: 0,
                y: 0,
                width: coded_size.width,
                height: coded_size.height,
            }),
            coded_size: Some(coded_size),
            frame_rate: Some(frame_rate),
            color: Some(ColorDescription {
                range: range as i32,
                primaries: ColorPrimaries::Bt709 as i32,
                transfer: ColorTransfer::Bt709 as i32,
                matrix: ColorMatrix::Bt709 as i32,
            }),
        }
    }

    pub fn validate(&self) -> Result<(), MediaFormatError> {
        let codec =
            VideoCodec::try_from(self.codec).map_err(|_| MediaFormatError("unknown codec"))?;
        let profile = VideoProfile::try_from(self.profile)
            .map_err(|_| MediaFormatError("unknown profile"))?;
        if !matches!(
            (codec, profile),
            (VideoCodec::Avc, VideoProfile::AvcHigh) | (VideoCodec::Hevc, VideoProfile::HevcMain)
        ) {
            return Err(MediaFormatError("codec/profile combination"));
        }
        if self.bit_depth != 8 || self.chroma != VideoChroma::Yuv420 as i32 {
            return Err(MediaFormatError("requires 8-bit 4:2:0"));
        }
        let size = self
            .coded_size
            .as_ref()
            .ok_or(MediaFormatError("missing coded size"))?;
        if !matches!((size.width, size.height), (1280, 720) | (1920, 1080 | 1088)) {
            return Err(MediaFormatError("unsupported coded size"));
        }
        let rect = self
            .visible_rect
            .as_ref()
            .ok_or(MediaFormatError("missing visible rect"))?;
        if !matches!((rect.width, rect.height), (1280, 720) | (1920, 1080))
            || [rect.x, rect.y, rect.width, rect.height]
                .iter()
                .any(|v| v % 2 != 0)
            || rect
                .x
                .checked_add(rect.width)
                .is_none_or(|end| end > size.width)
            || rect
                .y
                .checked_add(rect.height)
                .is_none_or(|end| end > size.height)
        {
            return Err(MediaFormatError("visible rect outside 4:2:0 coded image"));
        }
        let fps = self
            .frame_rate
            .as_ref()
            .ok_or(MediaFormatError("missing frame rate"))?;
        if fps.denominator != 1 || !matches!(fps.numerator, 30 | 60) {
            return Err(MediaFormatError(
                "requires explicit 30/1 or 60/1 frame rate",
            ));
        }
        let color = self
            .color
            .as_ref()
            .ok_or(MediaFormatError("missing color description"))?;
        if !matches!(
            ColorRange::try_from(color.range),
            Ok(ColorRange::Limited | ColorRange::Full)
        ) || color.primaries != ColorPrimaries::Bt709 as i32
            || color.transfer != ColorTransfer::Bt709 as i32
            || color.matrix != ColorMatrix::Bt709 as i32
        {
            return Err(MediaFormatError(
                "requires explicit BT.709 SDR color description",
            ));
        }
        Ok(())
    }
    fn minimum_level_idc(&self) -> u32 {
        // Called only after validate(): geometry/rate/codec are present and bounded.
        let size = self.coded_size.as_ref().unwrap();
        let fps = self.frame_rate.as_ref().unwrap();
        match (
            VideoCodec::try_from(self.codec).unwrap(),
            if size.height == 1088 {
                1080
            } else {
                size.height
            },
            fps.numerator,
        ) {
            (VideoCodec::Avc, 720, 30) => 31,
            (VideoCodec::Avc, 720, 60) => 32,
            (VideoCodec::Avc, 1080, 30) => 40,
            (VideoCodec::Avc, 1080, 60) => 42,
            (VideoCodec::Hevc, 720, 30) => 93,
            (VideoCodec::Hevc, 720, 60) | (VideoCodec::Hevc, 1080, 30) => 120,
            (VideoCodec::Hevc, 1080, 60) => 123,
            _ => unreachable!("validated product format"),
        }
    }

    fn accepts_level(&self, level: u32) -> bool {
        let known = match VideoCodec::try_from(self.codec) {
            Ok(VideoCodec::Avc) => matches!(level, 31 | 32 | 40 | 41 | 42 | 50 | 51 | 52),
            Ok(VideoCodec::Hevc) => matches!(level, 93 | 120 | 123 | 150 | 153 | 156),
            _ => false,
        };
        known && level >= self.minimum_level_idc()
    }
}

impl Capabilities {
    pub fn validate(&self) -> Result<(), MediaFormatError> {
        if self.offers.is_empty() || self.offers.len() > MAX_DECODER_OFFERS {
            return Err(MediaFormatError("decoder offer count"));
        }
        for (index, offer) in self.offers.iter().enumerate() {
            let format = offer
                .format
                .as_ref()
                .ok_or(MediaFormatError("missing offer format"))?;
            format.validate()?;
            if !format.accepts_level(offer.max_level_idc) {
                return Err(MediaFormatError("decoder level ceiling"));
            }
            if offer.max_access_unit_bytes == 0
                || offer.max_access_unit_bytes > MAX_MEDIA_ACCESS_UNIT_BYTES
            {
                return Err(MediaFormatError("access unit budget"));
            }
            if self.offers[..index]
                .iter()
                .any(|other| other.format == offer.format)
            {
                return Err(MediaFormatError("duplicate format"));
            }
        }
        Ok(())
    }

    /// Exact combination membership, including color and visible image geometry.
    pub fn supports(&self, format: &VideoFormat, level_idc: u32, access_unit_bytes: u32) -> bool {
        self.validate().is_ok()
            && format.validate().is_ok()
            && format.accepts_level(level_idc)
            && access_unit_bytes > 0
            && self.offers.iter().any(|offer| {
                offer.format.as_ref() == Some(format)
                    && level_idc <= offer.max_level_idc
                    && access_unit_bytes <= offer.max_access_unit_bytes
            })
    }
}
