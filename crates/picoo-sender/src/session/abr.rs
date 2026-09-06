use picoo_metrics::ReceiverStats as MetricsReceiverStats;
use picoo_protocol::control::{Capabilities, ReceiverStats as ReceiverStatsMsg};
use picoo_session::{HealthState, StreamState};
use picoo_transport::PicooTransport;

use super::SenderSession;

impl<T: PicooTransport> SenderSession<T> {
    /// Max height from receiver Capabilities (0 if unknown). REQ-PICOO-MEDIA-002.
    pub fn receiver_max_height(&self) -> u32 {
        self.receiver_capabilities
            .as_ref()
            .map_or(0, |caps| self.matching_decoder_height(caps))
    }

    fn matching_decoder_height(&self, caps: &Capabilities) -> u32 {
        // The current native Sender adapter is AVC. Dual-codec selection will
        // replace this adapter constraint when its native offers are available.
        let fps = self
            .pending_stream_config
            .as_ref()
            .map_or(30, |config| config.fps);
        caps.offers
            .iter()
            .filter_map(|offer| offer.format.as_ref())
            .filter(|format| format.codec == picoo_protocol::control::VideoCodec::Avc as i32)
            .filter(|format| {
                format
                    .frame_rate
                    .as_ref()
                    .is_some_and(|rate| rate.numerator == fps && rate.denominator == 1)
            })
            .filter_map(|format| format.coded_size.as_ref().map(|size| size.height))
            .max()
            .unwrap_or(0)
    }

    /// User / capability preferred capture height (does not change active encode height).
    pub fn set_preferred_height(&mut self, height: u32) -> bool {
        if !self.bitrate.set_preferred_height(height) {
            self.last_session_error = Some("UNSUPPORTED_SOURCE_HEIGHT".into());
            return false;
        }
        self.requested_preferred_height = height;
        true
    }

    /// Host thermal policy holds bitrate growth without changing source format.
    pub fn set_thermal_hold(&mut self, hold: bool) {
        self.bitrate.set_thermal_hold(hold);
    }

    pub fn thermal_hold(&self) -> bool {
        self.bitrate.thermal_hold()
    }

    pub(super) fn clear_receiver_capabilities(&mut self) {
        self.receiver_capabilities = None;
        self.bitrate
            .set_preferred_height(self.requested_preferred_height);
    }

    pub(super) fn apply_receiver_stats(&mut self, stats: ReceiverStatsMsg) {
        self.pre_fec_packet_loss = if stats.pre_fec_packet_loss.is_finite() {
            stats.pre_fec_packet_loss.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let local_link = self.transport.link_stats().unwrap_or_default();
        let metrics = MetricsReceiverStats {
            rtt_ms: stats.rtt_ms,
            packet_loss: stats.packet_loss,
            jitter_ms: stats.jitter_ms,
            reassembly_drop: stats.reassembly_drop,
            decoder_drop: stats.decoder_drop,
            frame_age_ms: stats.frame_age_ms,
            receive_bitrate: stats.receive_bitrate,
            jitter_buffer_target_ms: stats.jitter_buffer_target_ms,
            jitter_buffer_actual_delay_ms: stats.jitter_buffer_actual_delay_ms,
            jitter_buffer_occupancy_ms: stats.jitter_buffer_occupancy_ms,
            capture_to_encode_ms: stats.capture_to_encode_ms,
            encode_to_arrival_ms: stats.encode_to_arrival_ms,
            jitter_residence_ms: stats.jitter_residence_ms,
            decode_ms: stats.decode_ms,
            frame_publish_age_ms: stats.frame_publish_age_ms,
            end_to_end_latency_ms: stats.end_to_end_latency_ms,
            clock_uncertainty_ms: stats.clock_uncertainty_ms,
            receive_queue_age_ms: stats.receive_queue_age_ms,
            sender_queue_age_ms: local_link.video_queue_age_ms,
            sender_queue_dropped_access_units: local_link
                .video_dropped_access_units
                .saturating_add(self.pipeline.stats().dropped_access_units),
            sender_quic_lost_packets: local_link.lost_packets,
            sender_quic_sent_packets: local_link.sent_packets,
            sender_video_buffered_bytes: local_link.video_buffered_bytes,
        };
        self.last_receiver_stats = Some(metrics.clone());
        self.last_bitrate_action = self.bitrate.update(&metrics);
        // REQ-PICOO-SESSION-001: Network Unstable mirrors ARCH loss thresholds.
        if self.lifecycle.runtime.stream().is_streaming() {
            if metrics.packet_loss > 0.03 {
                self.lifecycle
                    .runtime
                    .set_health(HealthState::NetworkDegraded);
            } else if metrics.packet_loss < 0.01 {
                self.lifecycle.runtime.set_health(HealthState::Healthy);
            }
        }
    }

    #[doc(hidden)]
    pub fn apply_receiver_stats_for_test(&mut self, stats: ReceiverStatsMsg) {
        self.apply_receiver_stats(stats);
    }

    pub(super) fn handle_capabilities(&mut self, capabilities: Capabilities) -> bool {
        if capabilities.validate().is_err() {
            self.last_session_error = Some("INVALID_DECODER_CAPABILITIES".into());
            return false;
        }
        if self.matching_decoder_height(&capabilities) == 0 {
            self.last_session_error = Some("NO_MATCHING_DECODER_OFFER".into());
            return false;
        }
        self.receiver_capabilities = Some(capabilities);
        self.bitrate
            .set_preferred_height(self.requested_preferred_height);
        if self.lifecycle.runtime.stream() == StreamState::Negotiating {
            self.enter_streaming();
        }
        true
    }

    #[doc(hidden)]
    pub fn apply_capabilities_for_test(&mut self, capabilities: Capabilities) -> bool {
        self.handle_capabilities(capabilities)
    }
}
