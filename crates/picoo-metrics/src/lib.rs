//! Metrics types aligned with PCP/1 ReceiverStats — REQ-PICOO-PROTOCOL-006.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ReceiverStats {
    pub rtt_ms: f64,
    pub packet_loss: f64,
    pub jitter_ms: f64,
    pub reassembly_drop: u64,
    pub decoder_drop: u64,
    pub frame_age_ms: f64,
    pub receive_bitrate: u32,
    pub jitter_buffer_depth_ms: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StreamMetrics {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_bps: u32,
    pub latency_ms: f64,
    pub packet_loss: f64,
}
