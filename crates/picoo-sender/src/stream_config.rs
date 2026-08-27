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
            rotation: 0,
            mirrored: self.mirrored,
            color_range: "limited".into(),
            sps: self.sps.clone(),
            pps: self.pps.clone(),
            stream_epoch: self.stream_epoch,
        }
    }
}
