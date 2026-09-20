//! Complete decoder offers and bounded admission — REQ-PICOO-PROTOCOL-015.

use crate::control::{
    Capabilities, ColorDescription, ColorMatrix, ColorPrimaries, ColorRange, ColorTransfer,
    FrameRate, Resolution, VideoChroma, VideoCodec, VideoFormat, VideoProfile, VideoTier,
    VisibleRect,
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
            tier: match codec {
                VideoCodec::Avc => VideoTier::Avc,
                VideoCodec::Hevc => VideoTier::HevcMain,
                VideoCodec::Unspecified => VideoTier::Unspecified,
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
        if !matches!(
            (codec, VideoTier::try_from(self.tier)),
            (VideoCodec::Avc, Ok(VideoTier::Avc))
                | (
                    VideoCodec::Hevc,
                    Ok(VideoTier::HevcMain | VideoTier::HevcHigh)
                )
        ) {
            return Err(MediaFormatError("codec/tier combination"));
        }
        if self.bit_depth != 8 || self.chroma != VideoChroma::Yuv420 as i32 {
            return Err(MediaFormatError("requires 8-bit 4:2:0"));
        }
        let size = self
            .coded_size
            .as_ref()
            .ok_or(MediaFormatError("missing coded size"))?;
        if !matches!(
            (size.width, size.height),
            (1280, 720 | 736) | (1920, 1080 | 1088)
        ) {
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
        // ITU-T H.264 Annex A uses macroblocks; H.265 Annex A uses luma samples.
        // Count the actual coded workload, including storage padding.
        let (picture, limits): (u64, &[(u32, u64, u64)]) =
            match VideoCodec::try_from(self.codec).unwrap() {
                VideoCodec::Avc => (
                    u64::from(size.width.div_ceil(16)) * u64::from(size.height.div_ceil(16)),
                    &[
                        (31, 3600, 108_000),
                        (32, 5120, 216_000),
                        (40, 8192, 245_760),
                        (42, 8704, 522_240),
                    ],
                ),
                VideoCodec::Hevc => (
                    u64::from(size.width) * u64::from(size.height),
                    &[
                        (93, 983_040, 33_177_600),
                        (120, 2_228_224, 66_846_720),
                        (123, 2_228_224, 133_693_440),
                    ],
                ),
                VideoCodec::Unspecified => unreachable!("validated codec"),
            };
        limits
            .iter()
            .find(|(_, max_picture, max_rate)| {
                picture <= *max_picture && picture * u64::from(fps.numerator) <= *max_rate
            })
            .expect("bounded product coded size/rate")
            .0
    }

    fn accepts_level(&self, level: u32) -> bool {
        let known = match VideoCodec::try_from(self.codec) {
            Ok(VideoCodec::Avc) => matches!(level, 31 | 32 | 40 | 41 | 42 | 50 | 51 | 52),
            Ok(VideoCodec::Hevc) => matches!(level, 93 | 120 | 123 | 150 | 153 | 156),
            _ => false,
        };
        known
            && level >= self.minimum_level_idc()
            && (self.tier != VideoTier::HevcHigh as i32 || level >= 120)
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
