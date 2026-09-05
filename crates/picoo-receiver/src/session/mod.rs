//! Receiver session: listen, pump, teardown, jitter, and stats.
//!
//! REQ-PICOO-SESSION-001/002, REQ-PICOO-TRANSPORT-*, REQ-PICOO-PROTOCOL-006.

mod clock;
mod control;
mod decoder_worker;
mod health;
mod lifecycle;
#[cfg(any(test, feature = "loopback-diagnostics"))]
mod loopback;
mod media;
mod media_ingress;
mod pairing;
mod recovery;
mod reducer;
mod stats;
#[cfg(test)]
mod test_support;
mod transport_events;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(test)]
use bytes::Bytes;
use picoo_frame_hub::{
    FrameBufferPool, LatestFrameStore, PlaceholderMode, SharedFrameRingProducer,
};
use picoo_jitter::JitterBuffer;
use picoo_packet::{AssembledAccessUnit, ReassemblyMap};
use picoo_pairing::TrustedDeviceStore;
use picoo_protocol::control::{
    control_envelope::Payload as ControlPayload, ReceiverStats as ReceiverStatsMsg,
    SenderStats as SenderStatsMsg, StreamConfig,
};
use picoo_protocol::MAX_VIDEO_FRAGMENTS_PER_ACCESS_UNIT;
use picoo_session::{
    ConnectionState, NetworkHealthTracker, OutputState, SessionRuntimeState, StreamState,
    TrustState,
};
use picoo_transport::{Endpoint, QuicReceiverTransport, SessionId};

use crate::{IngressStats, ReceiverError, ReceiverIdentity};
use decoder_worker::{DecoderWorker, EncodedAccessUnit, FrameKind};
use pairing::{ActiveSender, PendingPairing};
use recovery::DecoderRecovery;
use recovery::RecoveryReason;
use reducer::{ReceiverCloseReason, ReceiverEvent, ReceiverReducerState};
use stats::{
    media_deadline_from_observations, observed_fragment_loss_ratio,
    playout_blocked_by_older_reassembly, InterarrivalJitter, StatsReporter,
};

#[cfg(any(test, feature = "loopback-diagnostics"))]
pub use loopback::{run_loopback_access_unit, run_paired_loopback_access_unit};
pub use picoo_pairing::{TrustedIdentityCandidate, TrustedIdentityReplacement};

pub struct ReceiverSession {
    runtime_wake: picoo_transport::TransportEventWake,
    transport: QuicReceiverTransport,
    reassembly: ReassemblyMap,
    latest_frame_store: LatestFrameStore,
    frame_buffer_pool: FrameBufferPool,
    identity: ReceiverIdentity,
    trusted: TrustedDeviceStore,
    trusted_store_path: Option<PathBuf>,
    active_sender: Option<ActiveSender>,
    pending_pairing: Option<PendingPairing>,
    lifecycle: ReceiverReducerState,
    network_health: NetworkHealthTracker,
    ingress: IngressStats,
    stats_reporter: StatsReporter,
    permit_unpaired_video: bool,
    /// When true (default), already-trusted senders skip short-code confirm (PUC-002).
    auto_accept_paired: bool,
    /// Idle placeholder style (PRD §16 / AC-D-SET-01).
    placeholder_mode: picoo_frame_hub::PlaceholderMode,
    shared_ring: Option<SharedFrameRingProducer>,
    current_stream_config: Option<Arc<StreamConfig>>,
    /// Newer-epoch datagrams may beat StreamConfig across QUIC channels.
    waiting_for_stream_config_epoch: Option<u32>,
    /// At most one complete future-generation IDR is retained until its
    /// reliable StreamConfig arrives; incomplete AUs never cross this gate.
    pending_stream_config_idr: Option<AssembledAccessUnit>,
    receiver_capabilities_sent: Option<()>,
    decoder_worker: DecoderWorker,
    /// Monotonic Worker completion revision used by deterministic tests and diagnostics.
    decoder_completions: u64,
    /// After peer disconnect, keep last frame this long before placeholder (REQ-PICOO-FRAME-005).
    last_frame_hold: Duration,
    placeholder_after: Option<Instant>,
    /// Complete-AU jitter buffer before decode (REQ-PICOO-SESSION-002).
    jitter: JitterBuffer,
    /// Network arrival variation, distinct from buffered media depth.
    interarrival_jitter: InterarrivalJitter,
    /// Receiver-local monotonic epoch used by adaptive playout timing.
    timing_origin: Instant,
    /// Last ReceiverStats payload sent to the sender (REQ-PICOO-PROTOCOL-006).
    last_stats: Option<picoo_metrics::ReceiverStats>,
    /// Latest Sender-local queue/path counters received on the reliable stream.
    last_sender_stats: Option<SenderStatsMsg>,
    /// Monotonic identity of the latest complete ReceiverStats window.
    /// Consumers use this to avoid counting the same one-second window twice.
    last_stats_revision: u64,
    /// Measured decoded LatestFrameStore output rate over the latest stats window.
    last_decoded_fps: u32,
    /// Max height advertised in Capabilities (MEDIA-002); default both 720+1080.
    advertised_max_height: u32,
    /// Most recent production decode failure, cleared after a real frame lands.
    last_media_error: Option<String>,
    decoder_recovery: DecoderRecovery,
    /// Sender-selected generation carried by every PCP ControlEnvelope.
    control_generation: Option<u64>,
    next_control_message_id: u64,
    last_received_control_message_id: u64,
    clock_sync: clock::ReceiverClockSync,
}

impl Default for ReceiverSession {
    fn default() -> Self {
        Self::new()
    }
}

impl ReceiverSession {
    pub fn new() -> Self {
        let runtime_wake = picoo_transport::TransportEventWake::default();
        Self {
            transport: QuicReceiverTransport::with_event_wake(runtime_wake.clone()),
            runtime_wake: runtime_wake.clone(),
            reassembly: ReassemblyMap::new(8, MAX_VIDEO_FRAGMENTS_PER_ACCESS_UNIT),
            latest_frame_store: LatestFrameStore::new(),
            frame_buffer_pool: FrameBufferPool::default(),
            identity: ReceiverIdentity::default(),
            trusted: TrustedDeviceStore::new(),
            trusted_store_path: None,
            active_sender: None,
            pending_pairing: None,
            lifecycle: ReceiverReducerState::default(),
            network_health: NetworkHealthTracker::default(),
            ingress: IngressStats::default(),
            stats_reporter: StatsReporter::new(),
            permit_unpaired_video: false,
            auto_accept_paired: true,
            placeholder_mode: PlaceholderMode::Logo,
            shared_ring: None,
            current_stream_config: None,
            waiting_for_stream_config_epoch: None,
            pending_stream_config_idr: None,
            receiver_capabilities_sent: None,
            decoder_worker: DecoderWorker::with_event_wake(runtime_wake),
            decoder_completions: 0,
            last_frame_hold: Duration::from_millis(500),
            placeholder_after: None,
            jitter: JitterBuffer::new(),
            interarrival_jitter: InterarrivalJitter::default(),
            timing_origin: Instant::now(),
            last_stats: None,
            last_sender_stats: None,
            last_stats_revision: 0,
            last_decoded_fps: 0,
            advertised_max_height: 1080,
            last_media_error: None,
            decoder_recovery: DecoderRecovery::new(),
            control_generation: None,
            next_control_message_id: 1,
            last_received_control_message_id: 0,
            clock_sync: clock::ReceiverClockSync::default(),
        }
    }

    pub fn runtime_wake(&self) -> picoo_transport::TransportEventWake {
        self.runtime_wake.clone()
    }

    /// Limit advertised Capabilities resolutions (REQ-PICOO-MEDIA-002). `720` or `1080`.
    pub fn set_advertised_max_height(&mut self, height: u32) {
        self.advertised_max_height = if height <= 720 { 720 } else { 1080 };
    }

    pub fn with_identity(mut self, identity: ReceiverIdentity) -> Self {
        self.identity = identity;
        self
    }

    pub fn identity(&self) -> &ReceiverIdentity {
        &self.identity
    }

    pub fn set_display_name(&mut self, display_name: impl Into<String>) {
        self.identity.set_display_name(display_name);
    }

    pub fn display_name(&self) -> &str {
        self.identity.display_name()
    }

    /// Used by GPUI desktop shell for live page sender label.
    #[allow(dead_code)]
    pub fn active_sender_summary(&self) -> Option<(String, String)> {
        self.active_sender
            .as_ref()
            .map(|s| (s.sender_id.clone(), s.device_name.clone()))
    }

    pub(crate) fn now_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    pub fn stream_config(&self) -> Option<&StreamConfig> {
        self.current_stream_config.as_deref()
    }

    #[cfg(any(test, feature = "loopback-diagnostics"))]
    pub fn set_permit_unpaired_video(&mut self, permit: bool) {
        self.permit_unpaired_video = permit;
    }

    /// Surface Virtual Camera Unavailable to UI (REQ-PICOO-SESSION-001 / PUC-004).
    /// Only applied while idle so an active session is not clobbered.
    pub fn mark_virtual_camera_unavailable(&mut self) {
        if self.lifecycle.runtime.output() != OutputState::PermissionRequired {
            self.lifecycle
                .runtime
                .set_output(OutputState::VirtualCameraUnavailable);
        }
    }

    /// Clear Virtual Camera Unavailable after install/repair (REQ-PICOO-SESSION-001).
    pub fn clear_virtual_camera_unavailable(&mut self) {
        if self.lifecycle.runtime.output() == OutputState::VirtualCameraUnavailable {
            self.lifecycle.runtime.set_output(OutputState::Ready);
        }
    }

    /// Surface permission gate to UI (REQ-PICOO-SESSION-001).
    pub fn mark_permission_required(&mut self) {
        self.lifecycle
            .runtime
            .set_output(OutputState::PermissionRequired);
    }

    pub fn ingress_stats(&self) -> IngressStats {
        self.ingress
    }

    pub fn last_media_error(&self) -> Option<&str> {
        self.last_media_error.as_deref()
    }

    /// Last ReceiverStats sent upstream (REQ-PICOO-PROTOCOL-006 / PUC-005 live metrics).
    pub fn last_stats(&self) -> Option<&picoo_metrics::ReceiverStats> {
        self.last_stats.as_ref()
    }

    /// Monotonic revision for [`Self::last_stats`]. A value is incremented only
    /// when a new complete stats window is produced and is never reset during
    /// reconnects in this ReceiverSession.
    pub fn last_stats_revision(&self) -> u64 {
        self.last_stats_revision
    }

    pub fn decoded_fps(&self) -> u32 {
        self.last_decoded_fps
    }

    pub fn is_connected(&self) -> bool {
        self.transport.is_connected()
    }

    pub fn latest_frame_store(&self) -> &LatestFrameStore {
        &self.latest_frame_store
    }

    pub fn bind_addr(&self) -> Option<std::net::SocketAddr> {
        self.transport.bind_addr()
    }

    pub fn runtime_state(&self) -> SessionRuntimeState {
        self.lifecycle.runtime
    }

    pub fn listen(&mut self, endpoint: Endpoint) -> Result<std::net::SocketAddr, ReceiverError> {
        let addr = self.transport.bind(endpoint)?;
        self.apply_receiver_event(ReceiverEvent::ListenerStarted)?;
        Ok(addr)
    }

    /// Time until the next media/session deadline when no transport, decoder,
    /// or command event arrives. The dedicated Receiver owner sleeps on the
    /// shared revision wake for at most this duration.
    pub fn next_wake_delay(&self) -> Duration {
        let now = Instant::now();
        let now_us = self.timing_origin.elapsed().as_micros() as u64;
        let maintenance = now + Duration::from_secs(1);
        let media_deadline = self.media_deadline();
        let playout_blocked = self
            .jitter
            .front_frame_id()
            .is_some_and(|candidate_frame_id| {
                playout_blocked_by_older_reassembly(
                    self.reassembly.oldest_unresolved_frame_id(),
                    candidate_frame_id,
                )
            });
        let max_queue_age_us = media_deadline.as_micros() as u64;
        let jitter_deadline = (if playout_blocked {
            self.jitter
                .next_expiration_delay_us(now_us, max_queue_age_us)
        } else {
            self.jitter.next_release_delay_us(now_us)
        })
        .map(|delay| now + Duration::from_micros(delay));
        let stats_deadline = self
            .lifecycle
            .runtime
            .stream()
            .is_streaming()
            .then_some(self.stats_reporter.last_sent + Duration::from_secs(1));
        [
            Some(maintenance),
            self.pending_pairing
                .as_ref()
                .map(|pending| pending.expires_at),
            self.reassembly.next_expiration_at(media_deadline),
            jitter_deadline,
            self.placeholder_after,
            stats_deadline,
            self.clock_sync.next_sync_at(),
            self.decoder_recovery.next_request_at(now),
        ]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(maintenance)
        .saturating_duration_since(now)
    }

    pub(super) fn expire_reassembly_deadline(&mut self) -> Result<(), ReceiverError> {
        let partial_drops_before = self.reassembly.partial_access_unit_drop_count();
        let gap_drops_before = self.reassembly.whole_access_unit_gap_drop_count();
        let media_deadline = self.media_deadline();
        self.reassembly
            .expire_incomplete_older_than(Instant::now(), media_deadline);
        self.ingress.reassembly_partial_access_unit_drops = self
            .ingress
            .reassembly_partial_access_unit_drops
            .saturating_add(
                self.reassembly
                    .partial_access_unit_drop_count()
                    .saturating_sub(partial_drops_before),
            );
        self.ingress.reassembly_whole_access_unit_gap_drops = self
            .ingress
            .reassembly_whole_access_unit_gap_drops
            .saturating_add(
                self.reassembly
                    .whole_access_unit_gap_drop_count()
                    .saturating_sub(gap_drops_before),
            );
        if self.reassembly.take_reference_chain_loss() {
            self.enter_decoder_recovery(RecoveryReason::ReferenceAccessUnitLost, true)?;
        }
        Ok(())
    }

    fn drain_jitter(&mut self) -> Result<(), ReceiverError> {
        let now_us = self.timing_origin.elapsed().as_micros() as u64;
        let max_queue_age_us = self.media_deadline().as_micros() as u64;
        if self.jitter.drop_expired(now_us, max_queue_age_us) {
            self.ingress.recovery_jitter_expired =
                self.ingress.recovery_jitter_expired.saturating_add(1);
            self.enter_decoder_recovery(RecoveryReason::ReferenceAccessUnitLate, true)?;
            return Ok(());
        }
        while let Some(candidate_frame_id) = self.jitter.front_frame_id() {
            // QUIC Datagrams can complete newer AUs before an older AU. Do not
            // advance the decoder prediction chain while that older AU is
            // still inside the reassembly deadline; expiry/FEC owns its final
            // outcome. Otherwise the older AU becomes falsely "late" merely
            // because two pipeline stages were scheduled independently.
            if playout_blocked_by_older_reassembly(
                self.reassembly.oldest_unresolved_frame_id(),
                candidate_frame_id,
            ) {
                break;
            }
            let Some(frame) = self.jitter.pop_ready(now_us) else {
                break;
            };
            self.publish_timeline_access_unit(EncodedAccessUnit {
                connection_generation: self
                    .transport
                    .active_session()
                    .map_or(0, |session| session.0),
                stream_generation: frame.stream_generation,
                frame_id: frame.frame_id,
                source_pts_us: frame.pts_us,
                encoded_at_us: frame.encoded_at_us,
                received_at_us: frame.received_at_us,
                decode_submitted_at_us: now_us,
                kind: if frame.keyframe {
                    FrameKind::Key
                } else if frame.discardable {
                    FrameKind::DiscardableDelta
                } else {
                    FrameKind::ReferenceDelta
                },
                data: frame.data,
            })?;
        }
        Ok(())
    }

    /// A deadline is a failure/recovery bound, not the normal playout target.
    /// It covers both the current playout budget and a network burst while
    /// remaining strictly bounded for interactive camera use.
    fn media_deadline(&self) -> Duration {
        let rtt_ms = self
            .transport
            .link_stats()
            .map_or(0.0, |stats| stats.rtt_ms.max(0.0));
        let frame_ms = self
            .current_stream_config
            .as_ref()
            .map_or(1_000.0 / 30.0, |config| {
                1_000.0 / f64::from(config.fps.max(1))
            });
        media_deadline_from_observations(
            rtt_ms,
            self.interarrival_jitter.milliseconds(),
            frame_ms,
            self.jitter.target_delay_ms(),
        )
    }

    fn maybe_send_receiver_stats(&mut self) -> Result<(), ReceiverError> {
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
            .latest_frame_store
            .latest()
            .map(|frame| {
                let now_us = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_micros() as u64)
                    .unwrap_or(0);
                now_us.saturating_sub(frame.timestamp_us) as f64 / 1000.0
            })
            .unwrap_or(0.0);
        let latency = self
            .latest_frame_store
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

    pub(crate) fn video_allowed(&self) -> bool {
        if self.permit_unpaired_video {
            return true;
        }
        self.active_sender
            .as_ref()
            .is_some_and(|sender| sender.video_allowed)
    }

    pub(crate) fn begin_streaming(&mut self, session: SessionId) {
        if !matches!(
            self.lifecycle.runtime.connection(),
            ConnectionState::Connected { .. }
        ) {
            self.lifecycle
                .runtime
                .set_connection(ConnectionState::Connected {
                    generation: session.0,
                });
        }
        self.lifecycle.runtime.set_trust(TrustState::Authenticated);
        let generation = self
            .current_stream_config
            .as_ref()
            .map_or(0, |config| config.stream_epoch);
        self.lifecycle
            .runtime
            .set_stream(StreamState::Streaming { generation });
    }

    pub(crate) fn send_control_payload(
        &mut self,
        session: SessionId,
        payload: picoo_protocol::control::control_envelope::Payload,
    ) -> Result<(), ReceiverError> {
        let generation = self.control_generation.ok_or_else(|| {
            ReceiverError::Protocol("control generation is not established".into())
        })?;
        let message_id = self.next_control_message_id;
        self.next_control_message_id = self.next_control_message_id.saturating_add(1);
        let out = picoo_protocol::encode_control_envelope(payload, message_id, generation);
        self.transport
            .send_control(session, out)
            .map_err(ReceiverError::Transport)
    }

    pub fn close(&mut self) {
        // REQ-PICOO-SESSION-009: tell the Sender this is an intentional user
        // stop before closing QUIC. A bare peer close is indistinguishable from
        // a network interruption and would correctly arm automatic reconnect.
        if let Some(session) = self.transport.active_session() {
            if self.video_allowed() {
                let _ = self.send_control_payload(
                    session,
                    picoo_protocol::control::control_envelope::Payload::StopStream(
                        picoo_protocol::control::StopStream {},
                    ),
                );
            }
        }
        // close is intentionally infallible for UI teardown, but decoder state
        // must never survive into a later session.
        let _ = self.apply_receiver_event(ReceiverEvent::UserClose);
    }

    fn reject_control_session(&mut self, session: SessionId) {
        let _ = self.apply_receiver_event(ReceiverEvent::AbortConnection {
            generation: session.0,
            reason: ReceiverCloseReason::InvalidControl,
        });
    }

    /// Test-only: shorten/extend last-frame hold before placeholder (REQ-PICOO-FRAME-005).
    #[cfg(test)]
    pub fn set_last_frame_hold_for_test(&mut self, hold: Duration) {
        self.last_frame_hold = hold;
    }

    /// Override adaptive playout for deterministic tests/loopback.
    /// `0` releases complete access units immediately.
    #[cfg(any(test, feature = "loopback-diagnostics"))]
    pub fn set_jitter_target_ms(&mut self, target_ms: u64) {
        self.jitter.set_fixed_target_ms(Some(target_ms));
    }

    /// Test-only: simulate peer disconnect without waiting on QUIC teardown.
    #[cfg(test)]
    pub fn inject_peer_disconnect_for_test(&mut self) -> Result<(), ReceiverError> {
        let retain_frame = self.lifecycle.runtime.stream().is_streaming()
            && self.latest_frame_store.latest().is_some()
            && !self.last_frame_hold.is_zero();
        let generation = self
            .transport
            .active_session()
            .map_or(1, |session| session.0);
        if self.transport.active_session().is_none() {
            self.lifecycle.active_generation = Some(generation);
            self.lifecycle.resources_active = true;
        }
        self.apply_receiver_event(ReceiverEvent::TransportDisconnected {
            generation,
            retain_frame,
        })?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn activate_connection_for_test(
        &mut self,
        generation: u64,
    ) -> Result<(), ReceiverError> {
        self.apply_receiver_event(ReceiverEvent::TransportConnected { generation })?;
        Ok(())
    }

    /// Test-only: inject a sender-originated control blob into the pairing/session handler.
    #[cfg(test)]
    pub fn inject_control_for_test(&mut self, msg: Bytes) -> Result<(), ReceiverError> {
        let session = self
            .transport
            .active_session()
            .ok_or_else(|| ReceiverError::Protocol("no active session".into()))?;
        self.handle_control(session, msg)
    }

    #[cfg(test)]
    pub fn inject_control_payload_for_test(
        &mut self,
        payload: picoo_protocol::control::control_envelope::Payload,
    ) -> Result<(), ReceiverError> {
        let session = self
            .transport
            .active_session()
            .ok_or_else(|| ReceiverError::Protocol("no active session".into()))?;
        let generation = self.control_generation.unwrap_or(session.0);
        let message = picoo_protocol::encode_control_envelope(
            payload,
            self.last_received_control_message_id.saturating_add(1),
            generation,
        );
        let result = self.handle_control(session, message);
        if result.is_err() {
            self.reject_control_session(session);
        }
        result
    }
}
