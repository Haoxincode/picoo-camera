//! Receiver media telemetry and sender feedback.

use super::{stats::observed_fragment_loss_ratio, ReceiverSession};
use crate::ReceiverError;
use picoo_protocol::control::{
    control_envelope::Payload as ControlPayload, ReceiverStats as ReceiverStatsMsg,
};
use std::time::Instant;

impl ReceiverSession {
    pub(super) fn maybe_send_receiver_stats(&mut self) -> Result<(), ReceiverError> {
        if !self.lifecycle.runtime.stream().is_streaming() {
            return Ok(());
        }
        if !self.stats_reporter.due() {
            return Ok(());
        }

        let session = self
            .transport
            .active_session()
            .ok_or(ReceiverError::NotListening)?;

        let elapsed = self
            .stats_reporter
            .last_sent
            .elapsed()
            .as_secs_f64()
            .max(0.001);
        let receive_bitrate = ((self.stats_reporter.window_bytes as f64 * 8.0) / elapsed) as u32;
        self.last_decoded_fps =
            (self.stats_reporter.window_decoded_frames as f64 / elapsed).round() as u32;
        let reassembly_drop = self
            .reassembly
            .drop_count()
            .saturating_sub(self.stats_reporter.last_reassembly_drops);
        let missing_fragments = self
            .reassembly
            .missing_fragment_count()
            .saturating_sub(self.stats_reporter.last_missing_fragments);
        let resolved_fragments = self
            .reassembly
            .resolved_fragment_count()
            .saturating_sub(self.stats_reporter.last_resolved_fragments);
        let fec_recovered_fragments = self
            .reassembly
            .fec_recovered_fragment_count()
            .saturating_sub(self.stats_reporter.last_fec_recovered_fragments);

        let receiver_now_us = self.timing_origin.elapsed().as_micros() as u64;
        let frame_age_ms = self
            .frames
            .latest()
            .map(|frame| {
                let now_us = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_micros() as u64)
                    .unwrap_or(0);
                #[cfg(not(any(target_os = "macos", windows)))]
                {
                    now_us.saturating_sub(frame.timestamp_us) as f64 / 1000.0
                }
                #[cfg(any(target_os = "macos", windows))]
                {
                    let _ = now_us;
                    frame.timeline().decoded_at.elapsed().as_secs_f64() * 1000.0
                }
            })
            .unwrap_or(0.0);
        let latency = self
            .frames
            .latest()
            .map(|frame| self.frame_latency_breakdown(frame, receiver_now_us))
            .unwrap_or_default();

        // REQ-PICOO-PROTOCOL-006: real RTT from Quinn path stats (via transport facade).
        let link = self.transport.link_stats().unwrap_or_default();
        // Quinn's `lost_packets / sent_packets` describes packets sent by this
        // endpoint. On Receiver those are control-stream packets, not incoming
        // Android video datagrams, so feeding that ratio into Sender ABR causes
        // false quality drops. Compare missing and received video fragments in
        // the same unit instead (REQ-PICOO-PROTOCOL-009).
        let packet_loss = observed_fragment_loss_ratio(resolved_fragments, missing_fragments);
        let pre_fec_packet_loss = observed_fragment_loss_ratio(
            resolved_fragments,
            missing_fragments.saturating_add(fec_recovered_fragments),
        );

        let jitter_timing = self.jitter.take_timing_stats();
        let stats = ReceiverStatsMsg {
            rtt_ms: link.rtt_ms,
            packet_loss,
            jitter_ms: self.interarrival_jitter.milliseconds(),
            reassembly_drop,
            decoder_drop: self.stats_reporter.window_decoder_drops,
            frame_age_ms,
            receive_bitrate,
            jitter_buffer_target_ms: jitter_timing.target_delay_ms,
            jitter_buffer_actual_delay_ms: jitter_timing.actual_delay_ms,
            jitter_buffer_occupancy_ms: jitter_timing.occupancy_ms,
            pre_fec_packet_loss,
            capture_to_encode_ms: latency.capture_to_encode_ms,
            encode_to_arrival_ms: latency.encode_to_arrival_ms,
            jitter_residence_ms: latency.jitter_residence_ms,
            decode_ms: latency.decode_ms,
            frame_publish_age_ms: latency.frame_publish_age_ms,
            end_to_end_latency_ms: latency.end_to_end_latency_ms,
            clock_uncertainty_ms: latency.clock_uncertainty_ms,
            receive_queue_age_ms: self.stats_reporter.window_max_receive_queue_age_ms,
        };

        let sender_stats = self.last_sender_stats.as_ref();
        self.last_stats = Some(picoo_metrics::ReceiverStats {
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
            sender_queue_age_ms: sender_stats.map_or(0.0, |stats| stats.video_queue_age_ms),
            sender_queue_dropped_access_units: sender_stats
                .map_or(0, |stats| stats.video_dropped_access_units),
            sender_quic_lost_packets: sender_stats.map_or(0, |stats| stats.quic_lost_packets),
            sender_quic_sent_packets: sender_stats.map_or(0, |stats| stats.quic_sent_packets),
            sender_video_buffered_bytes: sender_stats.map_or(0, |stats| stats.video_buffered_bytes),
        });
        self.last_stats_revision = self.last_stats_revision.saturating_add(1);

        tracing::info!(
            stats_revision = self.last_stats_revision,
            access_units = self.ingress.access_units,
            decoded_frames = self.ingress.decoded_frames,
            decoder_resets = self.ingress.decoder_resets,
            partial_access_unit_drops = self.ingress.reassembly_partial_access_unit_drops,
            whole_access_unit_gap_drops = self.ingress.reassembly_whole_access_unit_gap_drops,
            jitter_capacity_recoveries = self.ingress.recovery_jitter_capacity,
            arrived_after_playout_recoveries = self.ingress.recovery_arrived_after_playout,
            jitter_expired_recoveries = self.ingress.recovery_jitter_expired,
            fec_recovered_fragments = self.ingress.fec_recovered_fragments,
            packet_loss,
            pre_fec_packet_loss,
            rtt_ms = stats.rtt_ms,
            jitter_ms = stats.jitter_ms,
            target_ms = stats.jitter_buffer_target_ms,
            actual_delay_ms = stats.jitter_buffer_actual_delay_ms,
            occupancy_ms = stats.jitter_buffer_occupancy_ms,
            "receiver media window"
        );

        self.send_control_payload(session, ControlPayload::ReceiverStats(stats))?;

        // REQ-PICOO-SESSION-013: UI health uses slow episode hysteresis while
        // Sender ABR receives this raw window immediately above.
        self.observe_network_packet_loss(packet_loss);

        self.stats_reporter.last_sent = Instant::now();
        self.stats_reporter.window_bytes = 0;
        self.stats_reporter.window_decoder_drops = 0;
        self.stats_reporter.window_decoded_frames = 0;
        self.stats_reporter.window_max_receive_queue_age_ms = 0.0;
        self.stats_reporter.last_reassembly_drops = self.reassembly.drop_count();
        self.stats_reporter.last_missing_fragments = self.reassembly.missing_fragment_count();
        self.stats_reporter.last_resolved_fragments = self.reassembly.resolved_fragment_count();
        self.stats_reporter.last_fec_recovered_fragments =
            self.reassembly.fec_recovered_fragment_count();

        Ok(())
    }
}
