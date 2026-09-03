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
use picoo_jitter::JitterBuffer;
use picoo_media_decode::{create_platform_decoder, AccessUnitDecoder};
use picoo_packet::ReassemblyMap;
use picoo_pairing::TrustedDeviceStore;
use picoo_protocol::control::{
    ReceiverStats as ReceiverStatsMsg, SenderStats as SenderStatsMsg, StreamConfig,
};
use picoo_protocol::MAX_VIDEO_FRAGMENTS_PER_ACCESS_UNIT;
use picoo_session::ReceiverStatus;
use picoo_transport::{CloseReason, Endpoint, QuicReceiverTransport, SessionId, TransportEvent};
use prost::Message;

use crate::{IngressStats, ReceiverError, ReceiverIdentity};
use pairing::{ActiveSender, PendingPairing};
use recovery::DecoderRecovery;
use recovery::RecoveryReason;

pub use loopback::{run_loopback_access_unit, run_paired_loopback_access_unit};
pub use picoo_pairing::{TrustedIdentityCandidate, TrustedIdentityReplacement};

const MEDIA_DEADLINE_MIN_MS: f64 = 200.0;
const MEDIA_DEADLINE_MAX_MS: f64 = 300.0;

fn media_deadline_from_observations(
    rtt_ms: f64,
    jitter_ms: f64,
    frame_ms: f64,
    playout_target_ms: f64,
) -> Duration {
    // A failure boundary must sit materially beyond normal playout. Two
    // current playout budgets plus one source frame absorbs a single delayed
    // receiver/OS scheduling turn; RTT + 3*jitter + one frame remains the
    // independent network-burst bound. The hard cap still prevents latency
    // from growing without limit.
    let playout_bound_ms = 2.0 * playout_target_ms + frame_ms;
    let network_bound_ms = rtt_ms + 3.0 * jitter_ms + frame_ms;
    let deadline_ms = playout_bound_ms
        .max(network_bound_ms)
        .clamp(MEDIA_DEADLINE_MIN_MS, MEDIA_DEADLINE_MAX_MS);
    Duration::from_secs_f64(deadline_ms / 1_000.0)
}

struct StatsReporter {
    last_sent: Instant,
    window_bytes: u64,
    last_reassembly_drops: u64,
    last_missing_fragments: u64,
    last_resolved_fragments: u64,
    window_decoder_drops: u64,
    window_decoded_frames: u64,
}

/// RFC 3550-style inter-arrival jitter estimate without requiring synchronized
/// sender/receiver wall clocks. Both deltas are durations, so clock offset cancels.
#[derive(Default)]
struct InterarrivalJitter {
    last: Option<(Instant, u64)>,
    estimate_us: f64,
}

impl InterarrivalJitter {
    fn observe(&mut self, arrived_at: Instant, pts_us: u64) {
        let Some((last_arrival, last_pts_us)) = self.last else {
            self.last = Some((arrived_at, pts_us));
            return;
        };
        // Ignore an older AU that completed after a newer one. It will still be
        // ordered/dropped by JitterBuffer, but must not corrupt this estimator.
        if pts_us <= last_pts_us {
            return;
        }
        let arrival_delta_us = arrived_at.duration_since(last_arrival).as_micros() as f64;
        let pts_delta_us = pts_us.saturating_sub(last_pts_us) as f64;
        let variation_us = (arrival_delta_us - pts_delta_us).abs();
        self.estimate_us += (variation_us - self.estimate_us) / 16.0;
        self.last = Some((arrived_at, pts_us));
    }

    fn milliseconds(&self) -> f64 {
        self.estimate_us / 1_000.0
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod interarrival_jitter_tests {
    use super::InterarrivalJitter;
    use std::time::{Duration, Instant};

    #[test]
    fn stable_arrivals_have_zero_jitter_and_variation_uses_ewma() {
        let start = Instant::now();
        let mut jitter = InterarrivalJitter::default();
        jitter.observe(start, 1_000_000);
        jitter.observe(start + Duration::from_millis(33), 1_033_000);
        assert_eq!(jitter.milliseconds(), 0.0);

        jitter.observe(start + Duration::from_millis(86), 1_066_000);
        // 20ms variation / RFC-style gain 16 = 1.25ms.
        assert!((jitter.milliseconds() - 1.25).abs() < 0.001);
    }

    #[test]
    fn late_older_access_unit_does_not_corrupt_estimate() {
        let start = Instant::now();
        let mut jitter = InterarrivalJitter::default();
        jitter.observe(start, 100_000);
        jitter.observe(start + Duration::from_millis(40), 90_000);
        assert_eq!(jitter.milliseconds(), 0.0);
        jitter.observe(start + Duration::from_millis(33), 133_000);
        assert_eq!(jitter.milliseconds(), 0.0);
    }
}

impl StatsReporter {
    fn new() -> Self {
        Self {
            last_sent: Instant::now(),
            window_bytes: 0,
            last_reassembly_drops: 0,
            last_missing_fragments: 0,
            last_resolved_fragments: 0,
            window_decoder_drops: 0,
            window_decoded_frames: 0,
        }
    }

    fn record_packet(&mut self, payload_len: usize) {
        self.window_bytes += payload_len as u64;
    }

    fn record_decoder_drop(&mut self) {
        self.window_decoder_drops += 1;
    }

    fn record_decoded_frame(&mut self) {
        self.window_decoded_frames += 1;
    }

    fn due(&self) -> bool {
        self.last_sent.elapsed() >= Duration::from_secs(1)
    }
}

fn observed_fragment_loss_ratio(resolved_fragments: u64, missing_fragments: u64) -> f64 {
    if resolved_fragments == 0 {
        0.0
    } else {
        missing_fragments.min(resolved_fragments) as f64 / resolved_fragments as f64
    }
}

fn playout_blocked_by_older_reassembly(
    oldest_unresolved_frame_id: Option<u64>,
    candidate_frame_id: u64,
) -> bool {
    oldest_unresolved_frame_id
        .is_some_and(|unresolved_frame_id| unresolved_frame_id < candidate_frame_id)
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
    /// Measured decoded FrameHub output rate over the latest stats window.
    last_decoded_fps: u32,
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
                TransportEvent::VideoPackets(_, packets) => {
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
            self.publish_access_unit(frame.data, frame.keyframe)?;
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
            self.status == ReceiverStatus::Streaming && self.frame_hub.latest_ready().is_some();
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

        self.send_control_message(session, &stats)?;

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
        let _ = self.publish_waiting_placeholder();
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
}

#[cfg(test)]
mod stats_tests {
    use std::time::Duration;

    use super::{
        media_deadline_from_observations, observed_fragment_loss_ratio,
        playout_blocked_by_older_reassembly,
    };

    #[test]
    fn fragment_loss_compares_received_and_missing_fragments_in_the_same_unit() {
        assert_eq!(observed_fragment_loss_ratio(0, 0), 0.0);
        assert_eq!(observed_fragment_loss_ratio(10, 1), 0.1);
        assert_eq!(observed_fragment_loss_ratio(1, 1), 1.0);
    }

    #[test]
    fn newer_playout_waits_for_an_older_unresolved_access_unit() {
        assert!(playout_blocked_by_older_reassembly(Some(100), 200));
        assert!(!playout_blocked_by_older_reassembly(Some(200), 200));
        assert!(!playout_blocked_by_older_reassembly(Some(300), 200));
        assert!(!playout_blocked_by_older_reassembly(None, 200));
    }

    #[test]
    fn media_failure_deadline_stays_beyond_playout_and_is_hard_bounded() {
        assert_eq!(
            media_deadline_from_observations(20.0, 2.0, 33.0, 33.0),
            Duration::from_millis(200),
        );
        assert_eq!(
            media_deadline_from_observations(40.0, 20.0, 33.0, 80.0),
            Duration::from_millis(200),
        );
        assert_eq!(
            media_deadline_from_observations(150.0, 80.0, 33.0, 80.0),
            Duration::from_millis(300),
        );
    }
}
