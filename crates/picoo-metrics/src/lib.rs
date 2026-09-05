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
    /// Native source PTS to hardware encoder callback, in the Sender clock.
    pub capture_to_encode_ms: Option<f64>,
    /// Encoder callback to complete Receiver AU arrival after clock mapping.
    pub encode_to_arrival_ms: Option<f64>,
    /// Complete AU residence before Decoder Worker submission.
    pub jitter_residence_ms: Option<f64>,
    /// Decoder Worker submission to decoded frame completion.
    pub decode_ms: Option<f64>,
    /// Decoded frame completion to this metrics snapshot.
    pub frame_publish_age_ms: Option<f64>,
    /// Source PTS to this Receiver snapshot. Never populated before the
    /// generation-scoped affine clock mapping is stable.
    pub end_to_end_latency_ms: Option<f64>,
    pub clock_uncertainty_ms: Option<f64>,
    /// Maximum Receiver-local transport/event queue wait in this window.
    pub receive_queue_age_ms: f64,
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
