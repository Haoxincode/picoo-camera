//! StreamConfig helpers — REQ-PICOO-PROTOCOL-005.

use picoo_protocol::control::{StreamConfig, VideoProfile};
use picoo_rate_control::BitrateLadder;

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
    pub sps: Vec<u8>,
    pub pps: Vec<u8>,
}

impl Default for StreamConfigParams {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            fps: 30,
            bitrate_bps: BitrateLadder::for_height(720).unwrap().initial_bps,
            stream_epoch: 1,
            mirrored: false,
            rotation: 0,
            sps: Vec::new(),
            pps: Vec::new(),
        }
    }
}

impl StreamConfigParams {
    pub fn to_proto(&self) -> StreamConfig {
        let configuration =
            picoo_bitstream::CodecConfiguration::from_avc_parameter_sets(&self.sps, &self.pps).ok();
        let (profile, level_idc) = configuration
            .as_ref()
            .map_or((VideoProfile::Unspecified, 0), |config| {
                (VideoProfile::AvcHigh, u32::from(config.level_idc()))
            });
        StreamConfig {
            codec: picoo_protocol::control::VideoCodec::Avc as i32,
            profile: profile as i32,
            level_idc,
            width: self.width,
            height: self.height,
            fps: self.fps,
            bitrate: self.bitrate_bps,
            rotation: Self::normalize_rotation(self.rotation),
            mirrored: self.mirrored,
            color_range: picoo_protocol::control::ColorRange::Limited as i32,
            codec_configuration: configuration
                .map_or_else(Vec::new, |config| config.record().to_vec()),
            stream_epoch: self.stream_epoch,
        }
    }

    pub fn normalize_rotation(degrees: u32) -> u32 {
        match degrees % 360 {
            0 | 90 | 180 | 270 => degrees % 360,
            other => {
                // Snap to nearest quarter-turn for tolerant senders.
                let snapped = ((other as f64) / 90.0).round() as u32 * 90;
                snapped % 360
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_malformed_sps_never_invents_profile_or_level() {
        for sps in [
            vec![],
            vec![0x67, 100, 0],
            vec![0x68, 100, 0, 42],
            vec![0xe7, 100, 0, 42],
        ] {
            let config = StreamConfigParams {
                sps,
                ..Default::default()
            }
            .to_proto();
            assert_eq!(config.profile, VideoProfile::Unspecified as i32);
            assert_eq!(config.level_idc, 0);
        }
    }

    #[test]
    fn to_proto_carries_rotation() {
        let cfg = StreamConfigParams {
            rotation: 90,
            ..Default::default()
        };
        assert_eq!(cfg.to_proto().rotation, 90);
    }

    #[test]
    fn normalize_rotation_snaps_nearby_values() {
        assert_eq!(StreamConfigParams::normalize_rotation(0), 0);
        assert_eq!(StreamConfigParams::normalize_rotation(91), 90);
        assert_eq!(StreamConfigParams::normalize_rotation(200), 180);
        assert_eq!(StreamConfigParams::normalize_rotation(450), 90);
    }

    #[test]
    fn unsupported_profile_is_explicit_without_label_fallback() {
        let cfg = StreamConfigParams {
            sps: vec![0x67, 77, 0, 40, 0xaa],
            ..Default::default()
        };
        let proto = cfg.to_proto();
        assert_eq!(proto.profile, VideoProfile::Unspecified as i32);
        assert_eq!(proto.level_idc, 0);
    }

    #[test]
    fn stream_config_derives_high_profile_and_level_from_native_parameter_sets() {
        let (sps, pps) =
            picoo_bitstream::avc::extract_sps_pps(picoo_testkit::H264_1920X1080_RED_IDR).unwrap();
        let expected_level = u32::from(sps[3]);
        let cfg = StreamConfigParams {
            sps,
            pps,
            ..Default::default()
        };
        let proto = cfg.to_proto();
        assert_eq!(proto.profile, VideoProfile::AvcHigh as i32);
        assert_eq!(proto.level_idc, expected_level);
    }
}
