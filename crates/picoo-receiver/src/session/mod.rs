//! Receiver session: listen, pump, teardown, jitter, and stats.
//!
//! REQ-PICOO-SESSION-001/002, REQ-PICOO-TRANSPORT-*, REQ-PICOO-PROTOCOL-006.

mod control;
mod loopback;
mod media;
mod pairing;
mod recovery;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use bytes::Bytes;
use picoo_frame_hub::{FrameHub, PlaceholderMode, SharedFrameRingProducer};
use picoo_jitter::{Frame as JitterFrame, JitterBuffer, PushOutcome};
use picoo_media_decode::{create_platform_decoder, AccessUnitDecoder};
use picoo_packet::{ReassemblyError, ReassemblyMap};
use picoo_pairing::TrustedDeviceStore;
use picoo_protocol::control::{ReceiverStats as ReceiverStatsMsg, StreamConfig};
use picoo_protocol::MAX_VIDEO_FRAGMENTS_PER_ACCESS_UNIT;
use picoo_session::ReceiverStatus;
use picoo_transport::{CloseReason, Endpoint, QuicReceiverTransport, SessionId, TransportEvent};
use prost::Message;

use crate::{IngressStats, ReceiverError, ReceiverIdentity};
use pairing::{ActiveSender, PendingPairing};
use recovery::DecoderRecovery;
use recovery::RecoveryReason;

pub use loopback::{run_loopback_access_unit, run_paired_loopback_access_unit};

const REASSEMBLY_MAX_AGE: Duration = Duration::from_millis(120);

struct StatsReporter {
    last_sent: Instant,
    window_packets: u64,
    window_bytes: u64,
    last_reassembly_drops: u64,
    last_missing_fragments: u64,
    window_decoder_drops: u64,
}

impl StatsReporter {
    fn new() -> Self {
        Self {
            last_sent: Instant::now(),
            window_packets: 0,
            window_bytes: 0,
            last_reassembly_drops: 0,
            last_missing_fragments: 0,
            window_decoder_drops: 0,
        }
    }

    fn record_packet(&mut self, payload_len: usize) {
        self.window_packets += 1;
        self.window_bytes += payload_len as u64;
    }

    fn record_decoder_drop(&mut self) {
        self.window_decoder_drops += 1;
    }

    fn due(&self) -> bool {
        self.last_sent.elapsed() >= Duration::from_secs(1)
    }
}

fn observed_fragment_loss_ratio(received_fragments: u64, missing_fragments: u64) -> f64 {
    let observed_fragments = received_fragments.saturating_add(missing_fragments);
    if observed_fragments == 0 {
        0.0
    } else {
        missing_fragments as f64 / observed_fragments as f64
    }
}

pub struct ReceiverSession {
    transport: QuicReceiverTransport,
    reassembly: ReassemblyMap,
    frame_hub: FrameHub,
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
    /// Maps wall time onto the media PTS timeline for jitter scheduling.
    /// `(wall_anchor, pts_anchor)` — set on the first buffered AU of a burst.
    jitter_timeline: Option<(Instant, u64)>,
    /// Last ReceiverStats payload sent to the sender (REQ-PICOO-PROTOCOL-006).
    last_stats: Option<picoo_metrics::ReceiverStats>,
    /// Max height advertised in Capabilities (MEDIA-002); default both 720+1080.
    advertised_max_height: u32,
    /// Most recent production decode failure, cleared after a real frame lands.
    last_media_error: Option<String>,
    decoder_recovery: DecoderRecovery,
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
            frame_hub: FrameHub::new(),
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
            jitter: JitterBuffer::new(50, 120),
            jitter_timeline: None,
            last_stats: None,
            advertised_max_height: 1080,
            last_media_error: None,
            decoder_recovery: DecoderRecovery::new(),
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
        self.identity.display_name = display_name.into();
    }

    pub fn display_name(&self) -> &str {
        &self.identity.display_name
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

    pub fn is_connected(&self) -> bool {
        self.transport.is_connected()
    }

    pub fn frame_hub(&self) -> &FrameHub {
        &self.frame_hub
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
                TransportEvent::Connected(_) => {
                    self.placeholder_after = None;
                    self.status = ReceiverStatus::Connecting;
                }
                TransportEvent::Disconnected(_, _) => self.on_peer_disconnected()?,
                TransportEvent::ControlMessage(session, msg) => {
                    self.handle_control(session, msg)?;
                }
                TransportEvent::VideoPacket(session, packet) => {
                    // Enforce the wall-clock deadline before a queued late tail
                    // gets a chance to complete an already-expired AU.
                    self.expire_reassembly_deadline()?;
                    self.ingress.packets_received += 1;
                    if !self.video_allowed() {
                        self.ingress.packets_dropped_unpaired += 1;
                        continue;
                    }
                    let packet_epoch = packet.stream_epoch;
                    let configured_epoch = self
                        .current_stream_config
                        .as_ref()
                        .map(|config| config.stream_epoch);
                    if configured_epoch.is_some() && configured_epoch != Some(packet_epoch) {
                        // Stale datagrams from an old epoch are expected after
                        // reconfiguration. A future/unknown epoch waits for its
                        // reliable StreamConfig and requests one fresh IDR.
                        if configured_epoch.is_some_and(|epoch| packet_epoch > epoch)
                            && self.waiting_for_stream_config_epoch != Some(packet_epoch)
                        {
                            self.waiting_for_stream_config_epoch = Some(packet_epoch);
                            self.send_request_keyframe_now(session)?;
                        }
                        continue;
                    }
                    self.stats_reporter.record_packet(packet.payload.len());
                    match self.reassembly.ingest(packet) {
                        Ok(Some(access_unit)) => {
                            let pts_us = access_unit.pts_us;
                            let outcome = self.jitter.push(JitterFrame {
                                pts_us: access_unit.pts_us,
                                data: access_unit.data,
                                keyframe: access_unit.keyframe,
                                discardable: access_unit.discardable,
                            });
                            match outcome {
                                PushOutcome::Accepted if self.jitter_timeline.is_none() => {
                                    // Anchor media clock to this AU's PTS at wall arrival.
                                    self.jitter_timeline = Some((Instant::now(), pts_us));
                                }
                                PushOutcome::AcceptedAfterReferenceDrop
                                | PushOutcome::DroppedLate {
                                    requires_refresh: true,
                                } => {
                                    self.enter_decoder_recovery(
                                        RecoveryReason::ReferenceAccessUnitLate,
                                        true,
                                    )?;
                                }
                                PushOutcome::Accepted
                                | PushOutcome::DroppedLate {
                                    requires_refresh: false,
                                } => {}
                            }
                        }
                        Ok(None) => {}
                        // Reassembly owns drop/keyframe-loss accounting. Keep
                        // protocol rejects out of the decoder and continue the session.
                        Err(ReassemblyError::TooManyFragments)
                        | Err(ReassemblyError::DuplicateFragment)
                        | Err(ReassemblyError::EpochMismatch) => {}
                    }
                    if self.reassembly.take_reference_chain_loss() {
                        self.enter_decoder_recovery(RecoveryReason::ReferenceAccessUnitLost, true)?;
                    }
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
        self.reassembly
            .expire_incomplete_older_than(Instant::now(), REASSEMBLY_MAX_AGE);
        if self.reassembly.take_reference_chain_loss() {
            self.enter_decoder_recovery(RecoveryReason::ReferenceAccessUnitLost, true)?;
        }
        Ok(())
    }

    /// Media-clock "now" aligned with packet `pts_us` (REQ-PICOO-SESSION-002).
    ///
    /// JitterBuffer compares `now_us` against frame PTS; wall-clock UNIX time must
    /// not be passed in — relative media PTS would be treated as ancient and dropped.
    fn jitter_media_now_us(&self) -> u64 {
        match self.jitter_timeline {
            Some((wall_anchor, pts_anchor)) => {
                pts_anchor.saturating_add(wall_anchor.elapsed().as_micros() as u64)
            }
            None => 0,
        }
    }

    fn drain_jitter(&mut self) -> Result<(), ReceiverError> {
        if self.jitter.is_empty() {
            self.jitter_timeline = None;
            return Ok(());
        }
        let now_us = self.jitter_media_now_us();
        if self.jitter.drop_expired_before(now_us) {
            self.enter_decoder_recovery(RecoveryReason::ReferenceAccessUnitLate, true)?;
            return Ok(());
        }
        while let Some(frame) = self.jitter.pop_ready(now_us) {
            self.publish_access_unit(frame.data, frame.keyframe)?;
        }
        if self.jitter.is_empty() {
            self.jitter_timeline = None;
        }
        Ok(())
    }

    fn on_peer_disconnected(&mut self) -> Result<(), ReceiverError> {
        // Teardown must complete even if a platform decoder reports a flush
        // error; otherwise transport state from a dead peer can survive.
        let decoder_reset = self.decoder.reset();
        let had_live_frame =
            self.status == ReceiverStatus::Streaming && self.frame_hub.latest_ready().is_some();
        self.active_sender = None;
        self.pending_pairing = None;
        self.reassembly = ReassemblyMap::new(8, MAX_VIDEO_FRAGMENTS_PER_ACCESS_UNIT);
        self.stats_reporter = StatsReporter::new();
        self.jitter.clear();
        self.jitter_timeline = None;
        self.last_stats = None;
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
        let reassembly_drop = self
            .reassembly
            .drop_count()
            .saturating_sub(self.stats_reporter.last_reassembly_drops);
        let missing_fragments = self
            .reassembly
            .missing_fragment_count()
            .saturating_sub(self.stats_reporter.last_missing_fragments);

        let frame_age_ms = self
            .frame_hub
            .latest_ready()
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
        let window_packets = self.stats_reporter.window_packets;
        // Quinn's `lost_packets / sent_packets` describes packets sent by this
        // endpoint. On Receiver those are control-stream packets, not incoming
        // Android video datagrams, so feeding that ratio into Sender ABR causes
        // false quality drops. Compare missing and received video fragments in
        // the same unit instead (REQ-PICOO-PROTOCOL-009).
        let packet_loss = observed_fragment_loss_ratio(window_packets, missing_fragments);

        let stats = ReceiverStatsMsg {
            rtt_ms: link.rtt_ms,
            packet_loss,
            jitter_ms: self.jitter.depth_ms(),
            reassembly_drop,
            decoder_drop: self.stats_reporter.window_decoder_drops,
            frame_age_ms,
            receive_bitrate,
            jitter_buffer_depth_ms: self.jitter.depth_ms(),
        };

        self.last_stats = Some(picoo_metrics::ReceiverStats {
            rtt_ms: stats.rtt_ms,
            packet_loss: stats.packet_loss,
            jitter_ms: stats.jitter_ms,
            reassembly_drop: stats.reassembly_drop,
            decoder_drop: stats.decoder_drop,
            frame_age_ms: stats.frame_age_ms,
            receive_bitrate: stats.receive_bitrate,
            jitter_buffer_depth_ms: stats.jitter_buffer_depth_ms,
        });

        self.send_control_message(session, &stats)?;

        // REQ-PICOO-SESSION-001: reflect Network Unstable from live loss (ARCH >3% / <1%).
        if packet_loss > 0.03 {
            self.mark_network_unstable();
        } else if packet_loss < 0.01 {
            self.clear_network_unstable();
        }

        self.stats_reporter.last_sent = Instant::now();
        self.stats_reporter.window_packets = 0;
        self.stats_reporter.window_bytes = 0;
        self.stats_reporter.window_decoder_drops = 0;
        self.stats_reporter.last_reassembly_drops = self.reassembly.drop_count();
        self.stats_reporter.last_missing_fragments = self.reassembly.missing_fragment_count();

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

    pub(crate) fn send_control_message<M: Message>(
        &mut self,
        session: SessionId,
        message: &M,
    ) -> Result<(), ReceiverError> {
        let mut out = Vec::new();
        message
            .encode(&mut out)
            .map_err(|e| ReceiverError::Protocol(format!("encode control: {e}")))?;
        self.transport
            .send_control(session, Bytes::from(out))
            .map_err(ReceiverError::Transport)
    }

    pub fn close(&mut self) {
        // close is intentionally infallible for UI teardown, but decoder state
        // must never survive into a later session.
        let _ = self.decoder.reset();
        if self.transport.is_connected() {
            self.transport
                .close(picoo_transport::SessionId(1), CloseReason::LocalClose);
        }
        self.placeholder_after = None;
        self.status = ReceiverStatus::Disconnected;
        self.active_sender = None;
        self.pending_pairing = None;
        self.last_media_error = None;
        self.decoder_recovery.reset_session();
        let _ = self.publish_waiting_placeholder();
    }

    /// Test-only: shorten/extend last-frame hold before placeholder (REQ-PICOO-FRAME-005).
    #[cfg(test)]
    pub fn set_last_frame_hold_for_test(&mut self, hold: Duration) {
        self.last_frame_hold = hold;
    }

    /// Set jitter buffer target delay in milliseconds (REQ-PICOO-SESSION-002).
    /// `0` releases reassembled access units immediately (useful for tests/loopback).
    pub fn set_jitter_target_ms(&mut self, target_ms: u64) {
        self.jitter.set_target_ms(target_ms);
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
}

#[cfg(test)]
mod stats_tests {
    use super::observed_fragment_loss_ratio;

    #[test]
    fn fragment_loss_compares_received_and_missing_fragments_in_the_same_unit() {
        assert_eq!(observed_fragment_loss_ratio(0, 0), 0.0);
        assert_eq!(observed_fragment_loss_ratio(9, 1), 0.1);
        assert_eq!(observed_fragment_loss_ratio(0, 1), 1.0);
    }
}
