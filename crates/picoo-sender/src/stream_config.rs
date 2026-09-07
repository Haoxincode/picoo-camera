//! StreamConfig helpers — REQ-PICOO-PROTOCOL-005.

use picoo_bitstream::{Codec, CodecConfiguration};
use picoo_protocol::control::{StreamConfig, VideoProfile};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamConfigParams {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_bps: u32,
    pub stream_epoch: u32,
    pub mirrored: bool,
    /// Clockwise rotation degrees applied by Receiver/VCam (0/90/180/270).
    pub rotation: u32,
    pub configuration: std::sync::Arc<CodecConfiguration>,
}

impl StreamConfigParams {
    pub fn to_proto(&self) -> Result<StreamConfig, picoo_bitstream::BitstreamError> {
        if !matches!(self.fps, 30 | 60) || !matches!(self.rotation, 0 | 90 | 180 | 270) {
            return Err(picoo_bitstream::BitstreamError::Unsupported(
                "source frame rate or rotation",
            ));
        }
        let configuration = &self.configuration;
        configuration.validate_visible_size(self.width, self.height)?;
        if configuration.nal_length_size() != picoo_bitstream::NalLengthSize::Four {
            return Err(picoo_bitstream::BitstreamError::Unsupported(
                "wire requires four-byte NAL lengths",
            ));
        }
        let (codec, profile) = match configuration.codec() {
            Codec::Avc => (
                picoo_protocol::control::VideoCodec::Avc,
                VideoProfile::AvcHigh,
            ),
            Codec::Hevc => (
                picoo_protocol::control::VideoCodec::Hevc,
                VideoProfile::HevcMain,
            ),
        };
        Ok(StreamConfig {
            codec: codec as i32,
            profile: profile as i32,
            level_idc: u32::from(configuration.level_idc()),
            width: self.width,
            height: self.height,
            fps: self.fps,
            bitrate: self.bitrate_bps,
            rotation: self.rotation,
            mirrored: self.mirrored,
            color_range: picoo_protocol::control::ColorRange::Limited as i32,
            codec_configuration: configuration.record().to_vec(),
            stream_epoch: self.stream_epoch,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hevc() -> StreamConfigParams {
        StreamConfigParams {
            width: 64,
            height: 64,
            fps: 60,
            bitrate_bps: 3_000_000,
            stream_epoch: 7,
            mirrored: true,
            rotation: 90,
            configuration: CodecConfiguration::parse(
                Codec::Hevc,
                bytes::Bytes::from_static(include_bytes!(
                    "../../picoo-testkit/fixtures/hevc-64x64-bt709-config.bin"
                )),
            )
            .unwrap()
            .into(),
        }
    }

    #[test]
    fn native_hevc_record_cannot_be_labelled_avc_by_sender() {
        let source = hevc();
        let config = source.to_proto().unwrap();
        assert_eq!(
            config.codec,
            picoo_protocol::control::VideoCodec::Hevc as i32
        );
        assert_eq!(config.profile, VideoProfile::HevcMain as i32);
        assert_eq!(config.fps, 60);
        assert_eq!(config.rotation, 90);
        assert_eq!(config.stream_epoch, 7);
        let record =
            CodecConfiguration::parse(Codec::Hevc, config.codec_configuration.into()).unwrap();
        assert_eq!(&record, source.configuration.as_ref());
        assert_eq!(config.level_idc, u32::from(record.level_idc()));
    }

    #[test]
    fn declared_geometry_must_match_native_sps_before_serialization() {
        let mut source = hevc();
        source.width = 1280;
        source.height = 720;
        assert!(source.to_proto().is_err());
        source.width = 64;
        source.height = 64;
        assert!(source.to_proto().is_ok());
    }

    #[test]
    fn unsupported_source_attributes_are_rejected_without_rounding() {
        let mut source = hevc();
        source.rotation = 91;
        assert!(source.to_proto().is_err());
        source.rotation = 90;
        let mut record = source.configuration.record().to_vec();
        record[21] &= !3;
        source.configuration = CodecConfiguration::parse(Codec::Hevc, record.into())
            .unwrap()
            .into();
        assert!(source.to_proto().is_err());
        source = hevc();
        for fps in [0, 24, 120] {
            source.fps = fps;
            assert!(source.to_proto().is_err());
        }
    }
}
