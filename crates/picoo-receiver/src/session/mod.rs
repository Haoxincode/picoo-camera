//! Receiver session: listen, pump, teardown, jitter, and stats.
//!
//! REQ-PICOO-SESSION-001/002, REQ-PICOO-TRANSPORT-*, REQ-PICOO-PROTOCOL-006.

mod control;
mod loopback;
mod media;
mod pairing;
mod recovery;
mod stats;

use std::path::PathBuf;
use std::time::{Duration, Instant};

#[cfg(test)]
use bytes::Bytes;
use picoo_frame_hub::{LatestFrameStore, PlaceholderMode, SharedFrameRingProducer};
use picoo_jitter::JitterBuffer;
use picoo_media_decode::{create_platform_decoder, AccessUnitDecoder};
use picoo_packet::ReassemblyMap;
use picoo_pairing::TrustedDeviceStore;
use picoo_protocol::control::{
    control_envelope::Payload as ControlPayload, ReceiverStats as ReceiverStatsMsg,
    SenderStats as SenderStatsMsg, StreamConfig,
};
use picoo_protocol::MAX_VIDEO_FRAGMENTS_PER_ACCESS_UNIT;
use picoo_session::ReceiverStatus;
use picoo_transport::{CloseReason, Endpoint, QuicReceiverTransport, SessionId, TransportEvent};

use crate::{IngressStats, ReceiverError, ReceiverIdentity};
use pairing::{ActiveSender, PendingPairing};
use recovery::DecoderRecovery;
use recovery::RecoveryReason;
use stats::{
    media_deadline_from_observations, observed_fragment_loss_ratio,
    playout_blocked_by_older_reassembly, InterarrivalJitter, StatsReporter,
};

pub use loopback::{run_loopback_access_unit, run_paired_loopback_access_unit};
pub use picoo_pairing::{TrustedIdentityCandidate, TrustedIdentityReplacement};

pub struct ReceiverSession {
    transport: QuicReceiverTransport,
    reassembly: ReassemblyMap,
    latest_frame_store: LatestFrameStore,
    identity: ReceiverIdentity,
    trusted: TrustedDeviceStore,
    trusted_store_path: Option<PathBuf>,
    active_sender: Option<ActiveSender>,
    pending_pairing: Option<PendingPairing>,
    status: ReceiverStatus,
    ingress: IngressStats,
    stats_reporter: StatsReporter,
    permit_unpaired_video: bool,
    /// When true (default), already-trusted senders skip short-code confirm (PUC-002).
    auto_accept_paired: bool,
    /// Idle placeholder style (PRD §16 / AC-D-SET-01).
    placeholder_mode: picoo_frame_hub::PlaceholderMode,
    shared_ring: Option<SharedFrameRingProducer>,
    current_stream_config: Option<StreamConfig>,
    /// Newer-epoch datagrams may beat StreamConfig across QUIC channels.
    waiting_for_stream_config_epoch: Option<u32>,
    receiver_capabilities_sent: Option<()>,
    decoder: Box<dyn AccessUnitDecoder>,
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
}

impl Default for ReceiverSession {
    fn default() -> Self {
        Self::new()
    }
}

impl ReceiverSession {
    pub fn new() -> Self {
        Self {
            transport: QuicReceiverTransport::new(),
            reassembly: ReassemblyMap::new(8, MAX_VIDEO_FRAGMENTS_PER_ACCESS_UNIT),
            latest_frame_store: LatestFrameStore::new(),
            identity: ReceiverIdentity::default(),
            trusted: TrustedDeviceStore::new(),
            trusted_store_path: None,
            active_sender: None,
            pending_pairing: None,
            status: ReceiverStatus::Disconnected,
            ingress: IngressStats::default(),
            stats_reporter: StatsReporter::new(),
            permit_unpaired_video: false,
            auto_accept_paired: true,
            placeholder_mode: PlaceholderMode::Logo,
            shared_ring: None,
            current_stream_config: None,
            waiting_for_stream_config_epoch: None,
            receiver_capabilities_sent: None,
            decoder: create_platform_decoder(),
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
        }
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
        self.current_stream_config.as_ref()
    }

    pub fn set_permit_unpaired_video(&mut self, permit: bool) {
        self.permit_unpaired_video = permit;
    }

    pub fn status(&self) -> ReceiverStatus {
        self.status
    }

    /// Surface Virtual Camera Unavailable to UI (REQ-PICOO-SESSION-001 / PUC-004).
    /// Only applied while idle so an active session is not clobbered.
    pub fn mark_virtual_camera_unavailable(&mut self) {
        if matches!(
            self.status,
            ReceiverStatus::Discovering
                | ReceiverStatus::Disconnected
                | ReceiverStatus::VirtualCameraUnavailable
        ) {
            self.status = ReceiverStatus::VirtualCameraUnavailable;
        }
    }

    /// Clear Virtual Camera Unavailable after install/repair (REQ-PICOO-SESSION-001).
    pub fn clear_virtual_camera_unavailable(&mut self) {
        if self.status == ReceiverStatus::VirtualCameraUnavailable {
            self.status = if self.bind_addr().is_some() {
                ReceiverStatus::Discovering
            } else {
                ReceiverStatus::Disconnected
            };
        }
    }

    /// Surface permission gate to UI (REQ-PICOO-SESSION-001).
    pub fn mark_permission_required(&mut self) {
        self.status = ReceiverStatus::PermissionRequired;
    }

    /// Surface Network Unstable while live (REQ-PICOO-SESSION-001 / ARCH loss > 3%).
    pub fn mark_network_unstable(&mut self) {
        if matches!(
            self.status,
            ReceiverStatus::Streaming | ReceiverStatus::NetworkUnstable
        ) {
            self.status = ReceiverStatus::NetworkUnstable;
        }
    }

    /// Restore Streaming when loss recovers (REQ-PICOO-SESSION-001).
    pub fn clear_network_unstable(&mut self) {
        if self.status == ReceiverStatus::NetworkUnstable {
            self.status = ReceiverStatus::Streaming;
        }
    }

    pub fn ingress_stats(&self) -> IngressStats {
        self.ingress
    }

    pub fn last_media_error(&self) -> Option<&str> {
        self.last_media_error.as_deref()
    }

    /// Backward-compatible alias for ingress counters.
    pub fn stats(&self) -> IngressStats {
        self.ingress
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

    pub fn listen(&mut self, endpoint: Endpoint) -> Result<std::net::SocketAddr, ReceiverError> {
        let addr = self.transport.bind(endpoint)?;
        self.status = ReceiverStatus::Discovering;
        Ok(addr)
    }

    pub fn pump(&mut self) -> Result<(), ReceiverError> {
        self.expire_pending_pairing_if_needed();
        self.expire_reassembly_deadline()?;

        while let Some(event) = self.transport.poll_event() {
            match event {
                TransportEvent::Connected(session)
                    if self.transport.active_session() == Some(session) =>
                {
                    self.placeholder_after = None;
                    self.control_generation = None;
                    self.next_control_message_id = 1;
                    self.last_received_control_message_id = 0;
                    self.status = ReceiverStatus::Connecting;
                }
                TransportEvent::Disconnected(_, _) if self.transport.active_session().is_none() => {
                    self.on_peer_disconnected()?;
                }
                TransportEvent::ControlMessage(session, msg)
                    if self.transport.active_session() == Some(session) =>
                {
                    if let Err(error) = self.handle_control(session, msg) {
                        self.reject_control_session(session);
                        return Err(error);
                    }
                }
                TransportEvent::VideoPackets(session, packets)
                    if self.transport.active_session() == Some(session) =>
                {
                    for packet in packets {
                        self.ingest_video_packet(packet)?;
                    }
                    // The transport queue can remain continuously readable on
                    // a 1080p stream. Draining only after poll_event() becomes
                    // empty lets complete AUs accumulate behind an artificial
                    // scheduling boundary and can trip the jitter capacity
                    // guard even on a lossless LAN. Give every bounded ingress
                    // batch a playout opportunity before polling more media.
                    self.drain_jitter()?;
                }
                _ => {
                    // An event queued by an older connection generation must not
                    // mutate the currently active Receiver session.
                }
            }
        }

        // QUIC Datagram may reorder fragments across access units. A newer AU
        // is therefore not proof that an older partial AU was lost; only the
        // bounded real-time deadline makes that decision.
        self.expire_reassembly_deadline()?;

        self.drain_jitter()?;
        self.maybe_request_recovery_keyframe()?;
        self.maybe_finalize_disconnect_hold()?;
        self.maybe_send_receiver_stats()?;

        Ok(())
    }

    fn expire_reassembly_deadline(&mut self) -> Result<(), ReceiverError> {
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
            self.publish_timeline_access_unit(media::EncodedAccessUnit {
                stream_generation: frame.stream_generation,
                frame_id: frame.frame_id,
                source_pts_us: frame.pts_us,
                received_at_us: frame.received_at_us,
                kind: if frame.keyframe {
                    media::FrameKind::Key
                } else if frame.discardable {
                    media::FrameKind::DiscardableDelta
                } else {
                    media::FrameKind::ReferenceDelta
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

    fn on_peer_disconnected(&mut self) -> Result<(), ReceiverError> {
        // Teardown must complete even if a platform decoder reports a flush
        // error; otherwise transport state from a dead peer can survive.
        let decoder_reset = self.decoder.reset();
        let had_live_frame =
            self.status == ReceiverStatus::Streaming && self.latest_frame_store.latest().is_some();
        self.active_sender = None;
        self.pending_pairing = None;
        self.reassembly = ReassemblyMap::new(8, MAX_VIDEO_FRAGMENTS_PER_ACCESS_UNIT);
        self.stats_reporter = StatsReporter::new();
        self.jitter.clear();
        self.interarrival_jitter.reset();
        self.last_stats = None;
        self.last_sender_stats = None;
        self.last_decoded_fps = 0;
        self.last_media_error = None;
        self.current_stream_config = None;
        self.waiting_for_stream_config_epoch = None;
        self.receiver_capabilities_sent = None;
        self.decoder_recovery.reset_session();

        if had_live_frame && !self.last_frame_hold.is_zero() {
            // Briefly keep last frame for VCam/UI, then switch to placeholder.
            self.status = ReceiverStatus::Reconnecting;
            self.placeholder_after = Some(Instant::now() + self.last_frame_hold);
        } else {
            self.placeholder_after = None;
            let _ = self.publish_waiting_placeholder();
            self.status = if self.bind_addr().is_some() {
                ReceiverStatus::Discovering
            } else {
                ReceiverStatus::Disconnected
            };
        }
        decoder_reset?;
        Ok(())
    }

    fn maybe_finalize_disconnect_hold(&mut self) -> Result<(), ReceiverError> {
        let Some(deadline) = self.placeholder_after else {
            return Ok(());
        };
        if Instant::now() < deadline {
            return Ok(());
        }
        self.placeholder_after = None;
        // After last-frame hold, show reconnect copy before returning to idle Discovering.
        self.publish_reconnecting_placeholder()?;
        self.status = if self.bind_addr().is_some() {
            ReceiverStatus::Discovering
        } else {
            ReceiverStatus::Disconnected
        };
        Ok(())
    }

    fn maybe_send_receiver_stats(&mut self) -> Result<(), ReceiverError> {
        if !matches!(
            self.status,
            ReceiverStatus::Streaming | ReceiverStatus::NetworkUnstable
        ) {
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

        // REQ-PICOO-PROTOCOL-006: real RTT from Quinn path stats (via transport facade).
        let link = self.transport.link_stats().unwrap_or_default();
        // Quinn's `lost_packets / sent_packets` describes packets sent by this
        // endpoint. On Receiver those are control-stream packets, not incoming
        // Android video datagrams, so feeding that ratio into Sender ABR causes
        // false quality drops. Compare missing and received video fragments in
        // the same unit instead (REQ-PICOO-PROTOCOL-009).
        let packet_loss = observed_fragment_loss_ratio(resolved_fragments, missing_fragments);

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
            rtt_ms = stats.rtt_ms,
            jitter_ms = stats.jitter_ms,
            target_ms = stats.jitter_buffer_target_ms,
            actual_delay_ms = stats.jitter_buffer_actual_delay_ms,
            occupancy_ms = stats.jitter_buffer_occupancy_ms,
            "receiver media window"
        );

        self.send_control_payload(session, ControlPayload::ReceiverStats(stats))?;

        // REQ-PICOO-SESSION-001: reflect Network Unstable from live loss (ARCH >3% / <1%).
        if packet_loss > 0.03 {
            self.mark_network_unstable();
        } else if packet_loss < 0.01 {
            self.clear_network_unstable();
        }

        self.stats_reporter.last_sent = Instant::now();
        self.stats_reporter.window_bytes = 0;
        self.stats_reporter.window_decoder_drops = 0;
        self.stats_reporter.window_decoded_frames = 0;
        self.stats_reporter.last_reassembly_drops = self.reassembly.drop_count();
        self.stats_reporter.last_missing_fragments = self.reassembly.missing_fragment_count();
        self.stats_reporter.last_resolved_fragments = self.reassembly.resolved_fragment_count();

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

    pub(crate) fn begin_streaming(&mut self, _session: SessionId) -> Result<(), ReceiverError> {
        self.status = ReceiverStatus::Streaming;
        Ok(())
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
        // close is intentionally infallible for UI teardown, but decoder state
        // must never survive into a later session.
        let _ = self.decoder.reset();
        self.transport.close_active(CloseReason::LocalClose);
        self.placeholder_after = None;
        self.status = ReceiverStatus::Disconnected;
        self.active_sender = None;
        self.pending_pairing = None;
        self.reassembly = ReassemblyMap::new(8, MAX_VIDEO_FRAGMENTS_PER_ACCESS_UNIT);
        self.stats_reporter = StatsReporter::new();
        self.jitter.clear();
        self.interarrival_jitter.reset();
        self.last_stats = None;
        self.last_decoded_fps = 0;
        self.current_stream_config = None;
        self.waiting_for_stream_config_epoch = None;
        self.receiver_capabilities_sent = None;
        self.last_media_error = None;
        self.decoder_recovery.reset_session();
        self.control_generation = None;
        self.next_control_message_id = 1;
        self.last_received_control_message_id = 0;
        let _ = self.publish_waiting_placeholder();
    }

    fn reject_control_session(&mut self, session: SessionId) {
        self.transport.close(
            session,
            CloseReason::Error("invalid PCP control message".into()),
        );
        self.active_sender = None;
        self.pending_pairing = None;
        self.status = if self.bind_addr().is_some() {
            ReceiverStatus::Discovering
        } else {
            ReceiverStatus::Disconnected
        };
    }

    /// Test-only: shorten/extend last-frame hold before placeholder (REQ-PICOO-FRAME-005).
    #[cfg(test)]
    pub fn set_last_frame_hold_for_test(&mut self, hold: Duration) {
        self.last_frame_hold = hold;
    }

    /// Override adaptive playout for deterministic tests/loopback.
    /// `0` releases complete access units immediately.
    pub fn set_jitter_target_ms(&mut self, target_ms: u64) {
        self.jitter.set_fixed_target_ms(Some(target_ms));
    }

    /// Test-only: simulate peer disconnect without waiting on QUIC teardown.
    #[cfg(test)]
    pub fn inject_peer_disconnect_for_test(&mut self) -> Result<(), ReceiverError> {
        self.on_peer_disconnected()
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
