//! Metrics types aligned with PCP ReceiverStats — REQ-PICOO-PROTOCOL-006.

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
    pub jitter_buffer_target_ms: f64,
    pub jitter_buffer_actual_delay_ms: f64,
    pub jitter_buffer_occupancy_ms: f64,
    /// Sender-local complete-AU queue age, merged before ABR evaluation.
    pub sender_queue_age_ms: f64,
    /// Sender-local cumulative complete-AU queue drops.
    pub sender_queue_dropped_access_units: u64,
    /// Sender endpoint's cumulative QUIC packet-loss counter.
    pub sender_quic_lost_packets: u64,
    /// Sender endpoint's cumulative QUIC packet-send counter.
    pub sender_quic_sent_packets: u64,
    /// Sender's current queued QUIC Datagram payload, bounded by transport.
    pub sender_video_buffered_bytes: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StreamMetrics {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_bps: u32,
    pub latency_ms: f64,
    pub packet_loss: f64,
}
