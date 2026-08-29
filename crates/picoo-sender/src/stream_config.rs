//! StreamConfig helpers — REQ-PICOO-PROTOCOL-005.

use picoo_protocol::control::StreamConfig;

#[derive(Debug, Clone)]
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
            bitrate_bps: 6_000_000,
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
        let (profile, level) = self.h264_profile_level();
        StreamConfig {
            codec: "h264".into(),
            profile,
            level,
            width: self.width,
            height: self.height,
            fps: self.fps,
            bitrate: self.bitrate_bps,
            rotation: Self::normalize_rotation(self.rotation),
            mirrored: self.mirrored,
            color_range: "limited".into(),
            sps: self.sps.clone(),
            pps: self.pps.clone(),
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

    /// SPS is the codec source of truth. Platform encoders may fall back from
    /// Main to Baseline at runtime, so a hard-coded profile can disagree with
    /// the Access Units even when the FFI configuration is otherwise valid.
    fn h264_profile_level(&self) -> (String, String) {
        let sps = self.sps_payload();
        let Some(profile_idc) = sps.get(1).copied() else {
            return ("baseline".into(), "3.1".into());
        };
        let profile = match profile_idc {
            66 => "baseline",
            77 => "main",
            88 => "extended",
            100 => "high",
            110 => "high-10",
            122 => "high-4:2:2",
            244 => "high-4:4:4",
            _ => "unknown",
        };
        let level = sps
            .get(3)
            .map(|level_idc| format!("{}.{}", level_idc / 10, level_idc % 10))
            .unwrap_or_else(|| "3.1".into());
        (profile.into(), level)
    }

    fn sps_payload(&self) -> &[u8] {
        if self.sps.starts_with(&[0, 0, 0, 1]) {
            &self.sps[4..]
        } else if self.sps.starts_with(&[0, 0, 1]) {
            &self.sps[3..]
        } else {
            &self.sps
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn stream_config_derives_main_level_4_from_sps() {
        let cfg = StreamConfigParams {
            sps: vec![0x67, 77, 0, 40, 0xaa],
            ..Default::default()
        };
        let proto = cfg.to_proto();
        assert_eq!(proto.profile, "main");
        assert_eq!(proto.level, "4.0");
    }

    #[test]
    fn stream_config_derives_annex_b_baseline_from_sps() {
        let cfg = StreamConfigParams {
            sps: vec![0, 0, 0, 1, 0x67, 66, 0, 31],
            ..Default::default()
        };
        let proto = cfg.to_proto();
        assert_eq!(proto.profile, "baseline");
        assert_eq!(proto.level, "3.1");
    }
}
