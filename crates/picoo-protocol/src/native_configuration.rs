//! Actual bitstream facts for complete offer membership — REQ-PICOO-MEDIA-049.
use crate::control::{
    ColorRange, FrameRate, Resolution, StreamConfig, VideoCodec, VideoFormat, VisibleRect,
};
use crate::MediaFormatError;
use picoo_bitstream::{Codec, CodecConfiguration, NalLengthSize};

impl VideoFormat {
    /// Interpret actual configuration facts, without claiming native capability.
    pub fn from_codec_configuration(
        configuration: &CodecConfiguration,
        fps: u32,
    ) -> Result<Self, MediaFormatError> {
        if configuration.nal_length_size() != NalLengthSize::Four {
            return Err(MediaFormatError("requires four-byte NAL lengths"));
        }
        let facts = configuration
            .source_facts()
            .map_err(|_| MediaFormatError("invalid codec source facts"))?;
        let color = facts
            .color
            .ok_or(MediaFormatError("missing source color"))?;
        if (color.primaries, color.transfer, color.matrix) != (1, 1, 1)
            || facts.pixel_aspect_ratio.is_some_and(|(x, y)| x != y)
            || facts.chroma_location > 1
        {
            return Err(MediaFormatError("unsupported source presentation"));
        }
        let mut format = Self::sdr_709(
            match configuration.codec() {
                Codec::Avc => VideoCodec::Avc,
                Codec::Hevc => VideoCodec::Hevc,
            },
            Resolution {
                width: facts.coded_width,
                height: facts.coded_height,
            },
            FrameRate {
                numerator: fps,
                denominator: 1,
            },
            if color.full_range {
                ColorRange::Full
            } else {
                ColorRange::Limited
            },
        );
        if configuration.is_high_tier() {
            format.tier = crate::control::VideoTier::HevcHigh as i32;
        }
        format.visible_rect = Some(VisibleRect {
            x: facts.visible_x,
            y: facts.visible_y,
            width: facts.visible_width,
            height: facts.visible_height,
        });
        format.validate()?;
        Ok(format)
    }
}

impl StreamConfig {
    /// Validate wire labels against their authoritative standard codec record.
    pub fn validated_video_format(&self) -> Result<VideoFormat, MediaFormatError> {
        let codec = match VideoCodec::try_from(self.codec) {
            Ok(VideoCodec::Avc) => Codec::Avc,
            Ok(VideoCodec::Hevc) => Codec::Hevc,
            _ => return Err(MediaFormatError("unknown stream codec")),
        };
        let configuration =
            CodecConfiguration::parse(codec, self.codec_configuration.clone().into())
                .map_err(|_| MediaFormatError("invalid codec configuration"))?;
        let format = VideoFormat::from_codec_configuration(&configuration, self.fps)?;
        let visible = format
            .visible_rect
            .as_ref()
            .expect("validated visible rect");
        let color = format.color.as_ref().expect("validated color");
        if self.profile != format.profile
            || self.level_idc != u32::from(configuration.level_idc())
            || (self.width, self.height) != (visible.width, visible.height)
            || self.color_range != color.range
            || !matches!(self.rotation, 0 | 90 | 180 | 270)
        {
            return Err(MediaFormatError("stream labels differ from actual source"));
        }
        Ok(format)
    }
}

#[cfg(test)]
#[path = "native_configuration_tests.rs"]
mod tests;
