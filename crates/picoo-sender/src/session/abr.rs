use picoo_metrics::ReceiverStats as MetricsReceiverStats;
use picoo_protocol::control::{Capabilities, ReceiverStats as ReceiverStatsMsg};
use picoo_rate_control::{BitrateAction, BitrateLadder};
use picoo_session::{HealthState, StreamState};
use picoo_transport::PicooTransport;

use super::{EncoderDirective, EncoderDirectiveKind, SenderSession};

impl<T: PicooTransport> SenderSession<T> {
    /// Max height from receiver Capabilities (0 if unknown). REQ-PICOO-MEDIA-002.
    pub fn receiver_max_height(&self) -> u32 {
        self.receiver_capabilities
            .as_ref()
            .map(|caps| caps.resolutions.iter().map(|r| r.height).max().unwrap_or(0))
            .unwrap_or(0)
    }

    /// User / capability preferred capture height (does not change active encode height).
    pub fn set_preferred_height(&mut self, height: u32) {
        self.requested_preferred_height = picoo_rate_control::normalize_height(height);
        let preferred = self.cap_to_receiver_height(self.requested_preferred_height);
        self.bitrate.set_preferred_height(preferred);
    }

    /// Host thermal policy — block ABR upshift while overheating (MEDIA-010).
    pub fn set_thermal_hold(&mut self, hold: bool) {
        self.bitrate.set_thermal_hold(hold);
    }

    pub fn thermal_hold(&self) -> bool {
        self.bitrate.thermal_hold()
    }

    pub(super) fn queue_encoder_directive(
        &mut self,
        kind: EncoderDirectiveKind,
        target_height: u32,
    ) {
        if self.encoder_apply_state.is_applying() {
            return;
        }
        let target_height = self.cap_to_receiver_height(target_height);
        if target_height == self.bitrate.active_height() {
            let action = match kind {
                EncoderDirectiveKind::Local | EncoderDirectiveKind::Recovery => return,
                EncoderDirectiveKind::AbrDownshift => BitrateAction::DownshiftResolution,
                EncoderDirectiveKind::AbrUpshift => BitrateAction::UpshiftResolution,
            };
            self.bitrate.reject_resolution_change(action);
            return;
        }
        let id = self.next_encoder_directive_id;
        let Some(next_id) = id.checked_add(1) else {
            self.last_session_error = Some("ENCODER_DIRECTIVE_ID_EXHAUSTED".into());
            return;
        };
        let stream_epoch = self.allocate_stream_epoch();
        if stream_epoch == 0 {
            return;
        }
        let directive = EncoderDirective {
            id,
            kind,
            target_height,
            target_bitrate_bps: BitrateLadder::for_height(target_height).initial_bps,
            stream_epoch,
        };
        if !self.begin_encoder_transaction(directive) {
            return;
        }
        self.next_encoder_directive_id = next_id;
    }

    pub(super) fn cap_to_receiver_height(&self, height: u32) -> u32 {
        let requested = picoo_rate_control::normalize_height(height);
        let maximum = self.receiver_max_height();
        if maximum == 0 {
            requested
        } else {
            requested.min(picoo_rate_control::normalize_height(maximum))
        }
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
        if !self.encoder_apply_state.is_applying()
            && matches!(
                self.last_bitrate_action,
                BitrateAction::DownshiftResolution | BitrateAction::UpshiftResolution
            )
        {
            if let Some(target_height) = self.bitrate.target_height_for(self.last_bitrate_action) {
                let kind = match self.last_bitrate_action {
                    BitrateAction::DownshiftResolution => EncoderDirectiveKind::AbrDownshift,
                    BitrateAction::UpshiftResolution => EncoderDirectiveKind::AbrUpshift,
                    _ => unreachable!(),
                };
                self.queue_encoder_directive(kind, target_height);
            }
        }
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
        // Empty Capabilities is a prost false-positive for almost any blob.
        if !capabilities.codecs.is_empty() {
            self.receiver_capabilities = Some(capabilities);
            self.bitrate
                .set_preferred_height(self.cap_to_receiver_height(self.requested_preferred_height));
            if self.lifecycle.runtime.stream() == StreamState::Negotiating {
                self.enter_streaming();
            }
            true
        } else {
            false
        }
    }

    #[doc(hidden)]
    pub fn apply_capabilities_for_test(&mut self, capabilities: Capabilities) -> bool {
        self.handle_capabilities(capabilities)
    }
}
