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
mod media_publish;
mod media_report;
#[cfg(any(target_os = "macos", windows))]
mod native_publish;
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

#[cfg(any(target_os = "macos", windows))]
use crate::output::CpuOutput;
#[cfg(test)]
use bytes::Bytes;
#[cfg(any(target_os = "macos", windows))]
use picoo_frame_hub::FrameBus as SourceFrameStore;
use picoo_frame_hub::PlaceholderMode;
#[cfg(not(any(target_os = "macos", windows)))]
use picoo_frame_hub::{
    FrameBufferPool, LatestFrameStore as SourceFrameStore, SharedFrameRingWriter,
};

use picoo_jitter::JitterBuffer;
use picoo_packet::{AssembledAccessUnit, ReassemblyMap};
use picoo_pairing::TrustedDeviceStore;
use picoo_protocol::control::{SenderStats as SenderStatsMsg, StreamConfig};
use picoo_protocol::MAX_VIDEO_FRAGMENTS_PER_ACCESS_UNIT;
use picoo_session::{
    ConnectionState, NetworkHealthTracker, OutputState, SessionRuntimeState, StreamState,
    TrustState,
};
use picoo_transport::{Endpoint, QuicReceiverTransport, SessionId};

use crate::media_scheduler::{
    schedule_media, DecoderAdmission, MediaScheduleDecision, MediaScheduleInput, RecoveryAdmission,
};
use crate::{IngressStats, ReceiverError, ReceiverIdentity};
use decoder_worker::{DecoderWorker, EncodedAccessUnit, FrameKind};
use pairing::{ActiveSender, PendingPairing};
use recovery::DecoderRecovery;
use recovery::RecoveryReason;
use reducer::{ReceiverCloseReason, ReceiverEvent, ReceiverReducerState};
use stats::{media_deadline_from_observations, InterarrivalJitter, StatsReporter};

#[cfg(any(test, feature = "loopback-diagnostics"))]
pub use loopback::{run_loopback_access_unit, run_paired_loopback_access_unit};
pub use picoo_pairing::{TrustedIdentityCandidate, TrustedIdentityReplacement};

pub struct ReceiverSession {
    runtime_wake: picoo_transport::TransportEventWake,
    transport: QuicReceiverTransport,
    reassembly: ReassemblyMap,
    frames: SourceFrameStore,
    #[cfg(not(any(target_os = "macos", windows)))]
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
    #[cfg(not(any(target_os = "macos", windows)))]
    shared_ring: Option<SharedFrameRingWriter>,
    #[cfg(any(target_os = "macos", windows))]
    shared_ring: Option<CpuOutput>,
    last_shared_ring_error: Option<String>,
    current_stream_config: Option<Arc<StreamConfig>>,
    config_revision: u64,
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
            frames: SourceFrameStore::new(),
            #[cfg(not(any(target_os = "macos", windows)))]
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
            last_shared_ring_error: None,
            current_stream_config: None,
            config_revision: 0,
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

    pub fn last_shared_ring_error(&self) -> Option<&str> {
        self.last_shared_ring_error.as_deref()
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

    pub fn frames(&self) -> &SourceFrameStore {
        &self.frames
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
        let max_queue_age_us = media_deadline.as_micros() as u64;
        let media_wake = self
            .media_schedule_decision(now_us, max_queue_age_us)
            .wake_delay()
            .map(|delay| now + delay);
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
            media_wake,
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
        loop {
            match self.media_schedule_decision(now_us, max_queue_age_us) {
                MediaScheduleDecision::DiscardExpired => {
                    if self.jitter.drop_expired(now_us, max_queue_age_us) {
                        self.ingress.recovery_jitter_expired =
                            self.ingress.recovery_jitter_expired.saturating_add(1);
                        self.enter_decoder_recovery(RecoveryReason::ReferenceAccessUnitLate, true)?;
                        return Ok(());
                    }
                }
                MediaScheduleDecision::DecodeReadyFrame => {
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
                MediaScheduleDecision::DiscardReadyFrame => {
                    if self.jitter.pop_ready(now_us).is_none() {
                        break;
                    }
                    self.ingress.decoder_capacity_dropped_access_units = self
                        .ingress
                        .decoder_capacity_dropped_access_units
                        .saturating_add(1);
                }
                MediaScheduleDecision::DiscardRecoveryBlockedFrame => {
                    if self.jitter.pop_ready(now_us).is_none() {
                        break;
                    }
                    self.ingress.recovery_dropped_access_units =
                        self.ingress.recovery_dropped_access_units.saturating_add(1);
                }
                MediaScheduleDecision::WaitUntil { .. }
                | MediaScheduleDecision::WaitForEvent(_)
                | MediaScheduleDecision::Idle => break,
            }
        }
        Ok(())
    }

    fn media_schedule_decision(&self, now_us: u64, max_queue_age_us: u64) -> MediaScheduleDecision {
        let front = self.jitter.front_frame_descriptor();
        let connection_generation = self.control_generation.unwrap_or_else(|| {
            self.transport
                .active_session()
                .map_or(0, |session| session.0)
        });
        let recovery_admission = front.map_or(RecoveryAdmission::Ready, |frame| {
            self.decoder_recovery
                .admission(connection_generation, frame)
        });
        let decoder_admission = front.map_or(DecoderAdmission::Ready, |frame| {
            self.decoder_worker.admission(if frame.keyframe {
                FrameKind::Key
            } else if frame.discardable {
                FrameKind::DiscardableDelta
            } else {
                FrameKind::ReferenceDelta
            })
        });
        schedule_media(MediaScheduleInput {
            front_frame_id: self.jitter.front_frame_id(),
            oldest_unresolved_frame_id: self.reassembly.oldest_unresolved_frame_id(),
            release_delay: self
                .jitter
                .next_release_delay_us(now_us)
                .map(Duration::from_micros),
            expiration_delay: self
                .jitter
                .next_expiration_delay_us(now_us, max_queue_age_us)
                .map(Duration::from_micros),
            recovery_admission,
            decoder_admission,
        })
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
            && self.frames.latest().is_some()
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
