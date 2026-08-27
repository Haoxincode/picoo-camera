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
        StreamConfig {
            codec: "h264".into(),
            profile: "baseline".into(),
            level: "3.1".into(),
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
}
