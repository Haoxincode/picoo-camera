//! Desktop receiver session: QUIC ingress → reassembly → FrameHub.
//!
//! REQ-PICOO-FRAME-001, REQ-PICOO-MEDIA-005/006 via picoo-media-decode.
//! REQ-PICOO-PAIRING-*: ClientHello/ServerHello gate before video ingress.

pub const DEFAULT_SHARED_RING_NAME: &str = "picoo-camera-v1";

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bytes::Bytes;
use picoo_frame_hub::{
    FrameHub, FrameSlot, PlaceholderMode, SharedFrameRingProducer, PLACEHOLDER_HEIGHT,
    PLACEHOLDER_WIDTH,
};
use picoo_jitter::{Frame as JitterFrame, JitterBuffer, PushOutcome};
use picoo_media_decode::{create_platform_decoder, AccessUnitDecoder, DecodeError};
use picoo_packet::{ReassemblyError, ReassemblyMap};
use picoo_pairing::{
    new_pairing_challenge, pairing_transcript_hash, random_challenge_nonce,
    trusted_device_from_pairing, verify_pairing_confirm, PairingError, PairingHandshakeError,
    StoreError, TrustedDeviceStore,
};
use picoo_protocol::control::{
    camera_command, CameraCommand, Capabilities, ClientHello, EncoderCommand, PairingApproval,
    PairingChallenge as PairingChallengeMsg, PairingCommit, PairingComplete, PairingConfirm,
    ReceiverStats as ReceiverStatsMsg, Resolution, ServerHello, SessionError, StartStream,
    StopStream, StreamConfig,
};
use picoo_protocol::{ALPN, MAX_VIDEO_FRAGMENTS_PER_ACCESS_UNIT};
use picoo_session::ReceiverStatus;
use picoo_transport::{
    CloseReason, Endpoint, QuicReceiverTransport, SessionId, TransportError, TransportEvent,
};
use prost::Message;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ReceiverError {
    #[error("transport: {0}")]
    Transport(#[from] TransportError),
    #[error("sender: {0}")]
    Sender(#[from] picoo_sender::SenderError),
    #[error("frame hub: {0}")]
    FrameHub(#[from] picoo_frame_hub::FrameHubError),
    #[error("pairing: {0}")]
    Pairing(#[from] PairingError),
    #[error("protocol: {0}")]
    Protocol(String),
    #[error("shared ring: {0}")]
    SharedRing(#[from] picoo_frame_hub::SharedRingError),
    #[error("pairing store: {0}")]
    Store(#[from] StoreError),
    #[error("decode: {0}")]
    Decode(#[from] DecodeError),
    #[error("not listening")]
    NotListening,
    #[error("loopback timeout")]
    LoopbackTimeout,
}

#[derive(Debug, Clone)]
pub struct ReceiverIdentity {
    pub receiver_id: String,
    pub display_name: String,
    pub public_key: Vec<u8>,
}

impl Default for ReceiverIdentity {
    fn default() -> Self {
        Self {
            receiver_id: "windows-receiver".into(),
            display_name: "Picoo Camera".into(),
            public_key: vec![0x04, 0x01],
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IngressStats {
    pub access_units: u64,
    pub packets_received: u64,
    pub packets_dropped_unpaired: u64,
    /// Times the decoder was invoked (REQ-PICOO-MEDIA-006: once per AU).
    pub decode_invocations: u64,
    /// Frames successfully decoded and committed to FrameHub.
    pub decoded_frames: u64,
    /// StartStream / CameraCommand rejected while unpaired (PAIRING-003).
    pub control_rejected_unpaired: u64,
}

struct StatsReporter {
    last_sent: Instant,
    window_packets: u64,
    window_bytes: u64,
    last_reassembly_drops: u64,
    window_decoder_drops: u64,
}

impl StatsReporter {
    fn new() -> Self {
        Self {
            last_sent: Instant::now(),
            window_packets: 0,
            window_bytes: 0,
            last_reassembly_drops: 0,
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

struct ActiveSender {
    sender_id: String,
    device_name: String,
    public_key: Vec<u8>,
    video_allowed: bool,
}

struct PendingPairing {
    session: SessionId,
    challenge_nonce: Vec<u8>,
    short_code: String,
    local_confirmed: bool,
    remote_confirmed: bool,
    sender_committed: bool,
    receiver_committed: bool,
    /// PUC-001 / AC-M-PAIR-02: challenge valid for 60s (wall clock).
    expires_at: Instant,
}

/// Pairing short-code / challenge lifetime (matches Android PairingScreen TTL).
pub const PAIRING_CHALLENGE_TTL: Duration = Duration::from_secs(60);
const PAIRING_APPROVAL_MAGIC: u32 = 0x5041_5056;
const PAIRING_COMMIT_MAGIC: u32 = 0x5043_4D54;
const PAIRING_COMPLETE_MAGIC: u32 = 0x5043_4D50;
const PAIRING_APPROVAL_PHASE: &[u8] = b"pairing-approval-v2";
const PAIRING_COMMIT_PHASE: &[u8] = b"pairing-commit-v2";
const PAIRING_COMPLETE_PHASE: &[u8] = b"pairing-complete-v2";
const REASSEMBLY_MAX_AGE: Duration = Duration::from_millis(120);

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
        }
    }

    /// Limit advertised Capabilities resolutions (REQ-PICOO-MEDIA-002). `720` or `1080`.
    pub fn set_advertised_max_height(&mut self, height: u32) {
        self.advertised_max_height = if height <= 720 { 720 } else { 1080 };
    }

    /// Attach a cross-process Shared Frame Ring for VCam consumption (REQ-PICOO-FRAME-003).
    pub fn attach_shared_ring(&mut self, name: &str) -> Result<(), ReceiverError> {
        #[cfg(target_os = "macos")]
        let ring = if name == DEFAULT_SHARED_RING_NAME {
            let path = picoo_frame_hub::macos_app_group_ring_path(name)?;
            SharedFrameRingProducer::open_or_create_file(
                path,
                picoo_frame_hub::DEFAULT_MAX_FRAME_BYTES,
            )?
        } else {
            SharedFrameRingProducer::open_or_create(name, picoo_frame_hub::DEFAULT_MAX_FRAME_BYTES)?
        };
        #[cfg(not(target_os = "macos"))]
        let ring = SharedFrameRingProducer::open_or_create(
            name,
            picoo_frame_hub::DEFAULT_MAX_FRAME_BYTES,
        )?;
        self.shared_ring = Some(ring);
        self.publish_waiting_placeholder()?;
        Ok(())
    }

    pub fn publish_waiting_placeholder(&mut self) -> Result<(), ReceiverError> {
        let nv12 = self.placeholder_mode.waiting_frame();
        self.publish_nv12_frame(
            PLACEHOLDER_WIDTH,
            PLACEHOLDER_HEIGHT,
            PLACEHOLDER_WIDTH,
            0,
            0,
            &nv12,
        )
    }

    /// Publish reconnect-branded placeholder (REQ-PICOO-FRAME-005).
    pub fn publish_reconnecting_placeholder(&mut self) -> Result<(), ReceiverError> {
        let nv12 = self.placeholder_mode.reconnecting_frame();
        self.publish_nv12_frame(
            PLACEHOLDER_WIDTH,
            PLACEHOLDER_HEIGHT,
            PLACEHOLDER_WIDTH,
            0,
            0,
            &nv12,
        )
    }

    pub fn with_identity(mut self, identity: ReceiverIdentity) -> Self {
        self.identity = identity;
        self
    }

    pub fn identity(&self) -> &ReceiverIdentity {
        &self.identity
    }

    /// Prefer branded waiting frame (`true`) or solid black (`false`) — PRD §16.
    /// Prefer [`set_placeholder_mode`] for Logo / Black / Bars.
    pub fn set_use_default_placeholder(&mut self, enabled: bool) {
        self.placeholder_mode = if enabled {
            PlaceholderMode::Logo
        } else {
            PlaceholderMode::Black
        };
    }

    pub fn use_default_placeholder(&self) -> bool {
        matches!(self.placeholder_mode, PlaceholderMode::Logo)
    }

    pub fn set_placeholder_mode(&mut self, mode: PlaceholderMode) {
        self.placeholder_mode = mode;
    }

    pub fn placeholder_mode(&self) -> PlaceholderMode {
        self.placeholder_mode
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

    pub fn with_trusted_store(mut self, path: impl AsRef<Path>) -> Result<Self, ReceiverError> {
        let path = path.as_ref().to_path_buf();
        self.trusted = TrustedDeviceStore::load_from_path(&path)?;
        self.trusted_store_path = Some(path);
        Ok(self)
    }

    pub fn trusted_store_path(&self) -> Option<&Path> {
        self.trusted_store_path.as_deref()
    }

    pub fn remove_trusted_device(&mut self, device_id: &str) -> Result<bool, ReceiverError> {
        let removed = self.trusted.remove(device_id);
        if removed {
            self.persist_trusted()?;
        }
        Ok(removed)
    }

    /// Wipe all trusted devices; subsequent connects require re-pairing (PUC-007).
    pub fn clear_trusted_devices(&mut self) -> Result<usize, ReceiverError> {
        let n = self.trusted.clear();
        if n > 0 {
            self.persist_trusted()?;
        }
        Ok(n)
    }

    fn persist_trusted(&self) -> Result<(), ReceiverError> {
        if let Some(path) = &self.trusted_store_path {
            self.trusted.save_to_path(path)?;
        }
        Ok(())
    }

    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    pub fn stream_config(&self) -> Option<&StreamConfig> {
        self.current_stream_config.as_ref()
    }

    pub fn trusted_devices(&self) -> &TrustedDeviceStore {
        &self.trusted
    }

    pub fn trusted_devices_mut(&mut self) -> &mut TrustedDeviceStore {
        &mut self.trusted
    }

    /// Test/loopback helper — production receivers keep the default `false`.
    pub fn set_permit_unpaired_video(&mut self, permit: bool) {
        self.permit_unpaired_video = permit;
    }

    /// Auto-accept already-trusted senders without short-code (REQ-PICOO-UI-002 / PRD §16).
    pub fn set_auto_accept_paired(&mut self, enabled: bool) {
        self.auto_accept_paired = enabled;
    }

    pub fn auto_accept_paired(&self) -> bool {
        self.auto_accept_paired
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

    pub fn pairing_required(&self) -> bool {
        self.active_sender
            .as_ref()
            .is_some_and(|sender| !sender.video_allowed)
    }

    pub fn pairing_short_code(&self) -> Option<&str> {
        self.pending_pairing.as_ref().map(|p| p.short_code.as_str())
    }

    /// Remaining TTL for the active pairing challenge, if any.
    pub fn pairing_ttl_remaining(&self) -> Option<Duration> {
        let pending = self.pending_pairing.as_ref()?;
        Some(pending.expires_at.saturating_duration_since(Instant::now()))
    }

    /// Drop expired pending pairing (clears short code / modal).
    pub fn expire_pending_pairing_if_needed(&mut self) {
        let Some(pending) = self.pending_pairing.as_ref() else {
            return;
        };
        if Instant::now() < pending.expires_at {
            return;
        }
        self.pending_pairing = None;
        if matches!(self.status, ReceiverStatus::Pairing) {
            // Keep connection; UI must regenerate / wait for new challenge.
            self.status = ReceiverStatus::Connecting;
        }
    }

    /// User confirmed the six-digit code on desktop (PUC-001).
    pub fn confirm_pairing_locally(&mut self) -> Result<(), ReceiverError> {
        self.expire_pending_pairing_if_needed();
        if let Some(pending) = self.pending_pairing.as_mut() {
            pending.local_confirmed = true;
        }
        self.advance_pairing()
    }

    /// User explicitly rejected the active short-code challenge on desktop.
    ///
    /// The reliable SessionError lets mobile distinguish an intentional reject
    /// from an unrelated transport interruption (REQ-PICOO-PAIRING-001 /
    /// AC-M-PAIR-03).
    pub fn reject_pairing_locally(&mut self) -> Result<(), ReceiverError> {
        self.expire_pending_pairing_if_needed();
        let Some(pending) = self.pending_pairing.as_ref() else {
            return Ok(());
        };
        let session = pending.session;
        let error = SessionError {
            code: "PAIRING_REJECTED".into(),
            message: "desktop user rejected the pairing challenge".into(),
        };
        self.send_control_message(session, &error)?;
        self.transport.close(session, CloseReason::LocalClose);
        self.active_sender = None;
        self.pending_pairing = None;
        self.status = if self.bind_addr().is_some() {
            ReceiverStatus::Discovering
        } else {
            ReceiverStatus::Disconnected
        };
        Ok(())
    }

    pub fn is_awaiting_pairing_confirm(&self) -> bool {
        self.pending_pairing.is_some()
    }

    /// Test hook: expire the pending pairing challenge immediately.
    #[cfg(test)]
    pub fn force_expire_pending_pairing_for_test(&mut self) {
        if let Some(pending) = self.pending_pairing.as_mut() {
            pending.expires_at = Instant::now() - Duration::from_millis(1);
        }
        self.expire_pending_pairing_if_needed();
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
                            self.send_request_keyframe(session)?;
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
                            });
                            match outcome {
                                PushOutcome::Accepted if self.jitter_timeline.is_none() => {
                                    // Anchor media clock to this AU's PTS at wall arrival.
                                    self.jitter_timeline = Some((Instant::now(), pts_us));
                                }
                                PushOutcome::DroppedLate { keyframe: true } => {
                                    self.send_request_keyframe(session)?;
                                }
                                PushOutcome::Accepted
                                | PushOutcome::DroppedLate { keyframe: false } => {}
                            }
                        }
                        Ok(None) => {}
                        // Reassembly owns drop/keyframe-loss accounting. Keep
                        // protocol rejects out of the decoder and continue the session.
                        Err(ReassemblyError::TooManyFragments)
                        | Err(ReassemblyError::DuplicateFragment)
                        | Err(ReassemblyError::EpochMismatch) => {}
                    }
                    if self.reassembly.take_keyframe_loss() {
                        self.send_request_keyframe(session)?;
                    }
                }
            }
        }

        // QUIC Datagram may reorder fragments across access units. A newer AU
        // is therefore not proof that an older partial AU was lost; only the
        // bounded real-time deadline makes that decision.
        self.expire_reassembly_deadline()?;

        self.drain_jitter()?;
        self.maybe_finalize_disconnect_hold()?;
        self.maybe_send_receiver_stats()?;

        Ok(())
    }

    fn expire_reassembly_deadline(&mut self) -> Result<(), ReceiverError> {
        self.reassembly
            .expire_incomplete_older_than(Instant::now(), REASSEMBLY_MAX_AGE);
        if self.reassembly.take_keyframe_loss() {
            if let Some(session) = self.transport.active_session() {
                self.send_request_keyframe(session)?;
            }
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
        self.jitter.drop_incomplete_before(now_us);
        while let Some(frame) = self.jitter.pop_ready(now_us) {
            self.publish_access_unit(frame.data)?;
        }
        if self.jitter.is_empty() {
            self.jitter_timeline = None;
        }
        Ok(())
    }

    fn on_peer_disconnected(&mut self) -> Result<(), ReceiverError> {
        // Teardown must complete even if a platform decoder reports a flush
        // error; otherwise transport state from a dead peer can survive.
        let decoder_flush = self.decoder.flush();
        let had_live_frame =
            self.status == ReceiverStatus::Streaming && self.frame_hub.latest_ready().is_some();
        self.active_sender = None;
        self.pending_pairing = None;
        self.reassembly = ReassemblyMap::new(8, MAX_VIDEO_FRAGMENTS_PER_ACCESS_UNIT);
        self.jitter.clear();
        self.jitter_timeline = None;
        self.last_stats = None;
        self.last_media_error = None;
        self.current_stream_config = None;
        self.waiting_for_stream_config_epoch = None;
        self.receiver_capabilities_sent = None;

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
        decoder_flush?;
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
        let app_loss = if window_packets + reassembly_drop == 0 {
            0.0
        } else {
            reassembly_drop as f64 / (window_packets + reassembly_drop) as f64
        };
        // Quinn's `lost_packets / sent_packets` describes packets sent by this
        // endpoint. On Receiver those are control-stream packets, not incoming
        // Android video datagrams, so feeding that ratio into Sender ABR causes
        // false quality drops. Video health is measured at AU reassembly here.
        let packet_loss = app_loss;

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

        Ok(())
    }

    fn video_allowed(&self) -> bool {
        if self.permit_unpaired_video {
            return true;
        }
        self.active_sender
            .as_ref()
            .is_some_and(|sender| sender.video_allowed)
    }

    fn handle_control(&mut self, session: SessionId, msg: Bytes) -> Result<(), ReceiverError> {
        if self.pending_pairing.is_some() {
            if let Ok(commit) = PairingCommit::decode(msg.as_ref()) {
                if commit.magic == PAIRING_COMMIT_MAGIC
                    && self.pairing_transcript_matches(
                        session,
                        &commit.challenge_nonce,
                        &commit.transcript_hash,
                        PAIRING_COMMIT_PHASE,
                    )
                {
                    return self.handle_pairing_commit();
                }
            }
            // Prost will decode many unrelated blobs as PairingConfirm — require a
            // SHA-256-length signature that verifies against the pending challenge.
            if let Ok(confirm) = PairingConfirm::decode(msg.as_ref()) {
                if confirm.confirm_signature.len() == 32 {
                    if let Some(pending) = self.pending_pairing.as_ref() {
                        if session == pending.session {
                            let sender_id = self
                                .active_sender
                                .as_ref()
                                .map(|s| s.sender_id.as_str())
                                .unwrap_or("");
                            if verify_pairing_confirm(
                                &pending.challenge_nonce,
                                &self.identity.receiver_id,
                                sender_id,
                                &confirm.confirm_signature,
                            )
                            .is_ok()
                            {
                                return self.handle_pairing_confirm(session, msg);
                            }
                        }
                    }
                    // Unrelated control blob false-positive — keep waiting for real confirm.
                }
            }
            // PAIRING-003: StartStream during pending pairing must be rejected explicitly.
            if let Ok(start) = StartStream::decode(msg.as_ref()) {
                if start.magic == 1 {
                    return self.handle_start_stream(session);
                }
            }
            // StopStream must not wipe pairing; route through the same guard as post-pair.
            if let Ok(stop) = StopStream::decode(msg.as_ref()) {
                if stop.magic == 2 {
                    return self.handle_stop_stream(session);
                }
            }
            return Ok(());
        }
        if self.active_sender.is_none() {
            return self.handle_client_hello(session, msg);
        }
        // Discriminated control messages (magic/command != 0) before StreamConfig try-decode.
        if let Ok(start) = StartStream::decode(msg.as_ref()) {
            if start.magic == 1 {
                return self.handle_start_stream(session);
            }
        }
        if let Ok(stop) = StopStream::decode(msg.as_ref()) {
            if stop.magic == 2 {
                return self.handle_stop_stream(session);
            }
        }
        if let Ok(config) = StreamConfig::decode(msg.as_ref()) {
            // Require at least codec or dimensions so empty blobs are ignored.
            if !config.codec.is_empty() || config.width > 0 || config.height > 0 {
                return self.handle_stream_config(session, config);
            }
        }
        Ok(())
    }

    fn handle_start_stream(&mut self, session: SessionId) -> Result<(), ReceiverError> {
        if !self.video_allowed() {
            self.ingress.control_rejected_unpaired += 1;
            let err = SessionError {
                code: "UNPAIRED".into(),
                message: "StartStream rejected until pairing completes".into(),
            };
            let _ = self.send_control_message(session, &err);
            return Ok(());
        }
        self.begin_streaming(session)
    }

    fn handle_stop_stream(&mut self, session: SessionId) -> Result<(), ReceiverError> {
        // Unpaired / mid-pairing StopStream must not wipe the pairing challenge (PAIRING-003).
        if !self.video_allowed() {
            self.ingress.control_rejected_unpaired += 1;
            return Ok(());
        }
        // Finish protocol/session teardown before surfacing a decoder error.
        let decoder_flush = self.decoder.flush();
        // Sender-initiated stop: tear down session video without auto-reconnect wait.
        self.active_sender = None;
        self.pending_pairing = None;
        self.current_stream_config = None;
        self.waiting_for_stream_config_epoch = None;
        self.receiver_capabilities_sent = None;
        self.reassembly = ReassemblyMap::new(8, MAX_VIDEO_FRAGMENTS_PER_ACCESS_UNIT);
        self.jitter.clear();
        self.jitter_timeline = None;
        self.placeholder_after = None;
        let _ = self.publish_waiting_placeholder();
        self.transport.close(session, CloseReason::LocalClose);
        self.status = if self.bind_addr().is_some() {
            ReceiverStatus::Discovering
        } else {
            ReceiverStatus::Disconnected
        };
        decoder_flush?;
        Ok(())
    }

    /// Desktop → phone remote camera control (PUC-005).
    pub fn send_camera_command(&mut self, command: CameraCommand) -> Result<(), ReceiverError> {
        let session = self
            .transport
            .active_session()
            .ok_or(ReceiverError::NotListening)?;
        if !self.video_allowed() {
            self.ingress.control_rejected_unpaired += 1;
            return Err(ReceiverError::Protocol(
                "CameraCommand requires paired streaming session".into(),
            ));
        }
        if command.command == camera_command::Command::Unspecified as i32 {
            return Err(ReceiverError::Protocol("CameraCommand unspecified".into()));
        }
        self.send_control_message(session, &command)
    }

    fn handle_stream_config(
        &mut self,
        session: SessionId,
        config: StreamConfig,
    ) -> Result<(), ReceiverError> {
        let previous_epoch = self.current_stream_config.as_ref().map(|c| c.stream_epoch);
        let epoch_bumped = previous_epoch.is_some_and(|epoch| config.stream_epoch > epoch);
        self.current_stream_config = Some(config);
        self.waiting_for_stream_config_epoch = None;

        // Capability / StreamConfig exchange sits in Negotiating before live frames dominate UI.
        if self.video_allowed()
            && matches!(
                self.status,
                ReceiverStatus::Connecting
                    | ReceiverStatus::Pairing
                    | ReceiverStatus::Negotiating
                    | ReceiverStatus::Streaming
                    | ReceiverStatus::NetworkUnstable
            )
            && !matches!(
                self.status,
                ReceiverStatus::Streaming | ReceiverStatus::NetworkUnstable
            )
        {
            self.status = ReceiverStatus::Negotiating;
        }
        if self.receiver_capabilities_sent.is_none() {
            self.send_capabilities(session)?;
            self.receiver_capabilities_sent = Some(());
        }
        // After capabilities, paired receivers are ready to stream.
        if self.video_allowed() && self.status == ReceiverStatus::Negotiating {
            self.status = ReceiverStatus::Streaming;
        }

        // PUC-005 / REQ-PICOO-MEDIA-003 / SESSION-004: request IDR on first
        // StreamConfig and on every stream_epoch bump so decoders recover quickly.
        let needs_keyframe = self.video_allowed() && (previous_epoch.is_none() || epoch_bumped);
        if needs_keyframe {
            if epoch_bumped {
                self.jitter.clear();
                self.jitter_timeline = None;
                self.reassembly = ReassemblyMap::new(8, MAX_VIDEO_FRAGMENTS_PER_ACCESS_UNIT);
                self.decoder.flush()?;
            }
            self.send_request_keyframe(session)?;
        }
        Ok(())
    }

    fn send_capabilities(&mut self, session: SessionId) -> Result<(), ReceiverError> {
        // Advertise 480p / 720p / 1080p ladder (REQ-PICOO-UI-0001 AC-M-LIVE-01 + PUC-005).
        let mut resolutions = vec![
            Resolution {
                width: 854,
                height: 480,
            },
            Resolution {
                width: 1280,
                height: 720,
            },
        ];
        if self.advertised_max_height >= 1080 {
            resolutions.push(Resolution {
                width: 1920,
                height: 1080,
            });
        }
        let capabilities = Capabilities {
            codecs: vec!["h264".into()],
            resolutions,
            fps: vec![30],
            front_camera: true,
            back_camera: true,
        };
        self.send_control_message(session, &capabilities)
    }

    /// Ask Sender for an IDR after keyframe reassembly loss (REQ-PICOO-SESSION-003).
    fn send_request_keyframe(&mut self, session: SessionId) -> Result<(), ReceiverError> {
        let command = EncoderCommand {
            command: picoo_protocol::control::encoder_command::Command::RequestKeyframe as i32,
        };
        self.send_control_message(session, &command)
    }

    /// UI-triggered IDR request (REQ-PICOO-UI-003 live page).
    pub fn request_keyframe(&mut self) -> Result<(), ReceiverError> {
        let session = self
            .transport
            .active_session()
            .ok_or(ReceiverError::NotListening)?;
        if !self.video_allowed() {
            return Err(ReceiverError::Protocol(
                "RequestKeyframe requires paired streaming session".into(),
            ));
        }
        self.send_request_keyframe(session)
    }

    fn begin_streaming(&mut self, _session: SessionId) -> Result<(), ReceiverError> {
        self.status = ReceiverStatus::Streaming;
        Ok(())
    }

    fn handle_client_hello(&mut self, session: SessionId, msg: Bytes) -> Result<(), ReceiverError> {
        let hello = ClientHello::decode(msg.as_ref())
            .map_err(|e| ReceiverError::Protocol(format!("ClientHello decode: {e}")))?;

        // ARCH-PICOO-PROTOCOL-001: version negotiation must fail fast.
        if hello.protocol_version != picoo_protocol::ALPN {
            self.transport.close(session, CloseReason::LocalClose);
            return Err(ReceiverError::Protocol(format!(
                "unsupported protocol_version {:?} (expected {})",
                hello.protocol_version,
                picoo_protocol::ALPN
            )));
        }

        // PAIRING-004 / PUC-007: known device_id with changed public key → hard reject
        // (no pending re-pair, trust store unchanged; peer must remove + re-pair).
        if self.trusted.is_paired(&hello.sender_id)
            && self
                .trusted
                .verify_paired_key(&hello.sender_id, &hello.public_key)
                .is_err()
        {
            let err = SessionError {
                code: "PUBLIC_KEY_CHANGED".into(),
                message: "paired device public key changed; remove and re-pair".into(),
            };
            let _ = self.send_control_message(session, &err);
            self.transport.close(session, CloseReason::LocalClose);
            self.active_sender = None;
            self.pending_pairing = None;
            self.status = if self.bind_addr().is_some() {
                ReceiverStatus::Discovering
            } else {
                ReceiverStatus::Disconnected
            };
            return Ok(());
        }

        let paired = self
            .trusted
            .verify_paired_key(&hello.sender_id, &hello.public_key)
            .is_ok();
        let auto_accept = paired && self.auto_accept_paired;

        let server_hello = ServerHello {
            receiver_id: self.identity.receiver_id.clone(),
            display_name: self.identity.display_name.clone(),
            protocol_version: ALPN.into(),
            public_key: self.identity.public_key.clone(),
            pairing_required: !auto_accept,
        };
        self.send_control_message(session, &server_hello)?;

        if auto_accept {
            self.trusted
                .touch_last_connected(&hello.sender_id, self.now_ms());
            self.persist_trusted()?;
            self.active_sender = Some(ActiveSender {
                sender_id: hello.sender_id,
                device_name: hello.device_name,
                public_key: hello.public_key,
                video_allowed: true,
            });
            return self.begin_streaming(session);
        }

        let nonce = random_challenge_nonce();
        let challenge = new_pairing_challenge(&nonce, &self.identity.receiver_id, &hello.sender_id);
        let challenge_msg = PairingChallengeMsg {
            short_code: challenge.short_code.clone(),
            challenge_nonce: challenge.challenge_nonce,
        };
        self.send_control_message(session, &challenge_msg)?;

        self.pending_pairing = Some(PendingPairing {
            session,
            challenge_nonce: nonce,
            short_code: challenge.short_code,
            local_confirmed: false,
            remote_confirmed: false,
            sender_committed: false,
            receiver_committed: false,
            expires_at: Instant::now() + PAIRING_CHALLENGE_TTL,
        });
        self.active_sender = Some(ActiveSender {
            sender_id: hello.sender_id,
            device_name: hello.device_name,
            public_key: hello.public_key,
            video_allowed: false,
        });
        self.status = ReceiverStatus::Pairing;
        Ok(())
    }

    fn handle_pairing_confirm(
        &mut self,
        session: SessionId,
        msg: Bytes,
    ) -> Result<(), ReceiverError> {
        let confirm = PairingConfirm::decode(msg.as_ref())
            .map_err(|e| ReceiverError::Protocol(format!("PairingConfirm decode: {e}")))?;

        let pending = self
            .pending_pairing
            .as_ref()
            .ok_or_else(|| ReceiverError::Protocol("no pending pairing".into()))?;

        if Instant::now() >= pending.expires_at {
            self.pending_pairing = None;
            if matches!(self.status, ReceiverStatus::Pairing) {
                self.status = ReceiverStatus::Connecting;
            }
            return Err(ReceiverError::Protocol("pairing challenge expired".into()));
        }

        if session != pending.session {
            return Err(ReceiverError::Protocol("pairing session mismatch".into()));
        }

        let sender_id = self
            .active_sender
            .as_ref()
            .map(|s| s.sender_id.as_str())
            .unwrap_or("");

        verify_pairing_confirm(
            &pending.challenge_nonce,
            &self.identity.receiver_id,
            sender_id,
            &confirm.confirm_signature,
        )
        .map_err(|e| match e {
            PairingHandshakeError::InvalidSignature => {
                ReceiverError::Protocol("invalid pairing signature".into())
            }
        })?;

        if let Some(pending) = self.pending_pairing.as_mut() {
            pending.remote_confirmed = true;
        }
        self.advance_pairing()
    }

    fn pairing_transcript_matches(
        &self,
        session: SessionId,
        nonce: &[u8],
        transcript_hash: &[u8],
        phase: &[u8],
    ) -> bool {
        let Some(pending) = self.pending_pairing.as_ref() else {
            return false;
        };
        let Some(active) = self.active_sender.as_ref() else {
            return false;
        };
        session == pending.session
            && nonce == pending.challenge_nonce
            && transcript_hash
                == pairing_transcript_hash(
                    &pending.challenge_nonce,
                    &self.identity.receiver_id,
                    &active.sender_id,
                    phase,
                )
    }

    fn handle_pairing_commit(&mut self) -> Result<(), ReceiverError> {
        if let Some(pending) = self.pending_pairing.as_mut() {
            pending.sender_committed = true;
        }
        self.advance_pairing()
    }

    fn advance_pairing(&mut self) -> Result<(), ReceiverError> {
        let Some(pending) = self.pending_pairing.as_ref() else {
            return Ok(());
        };
        if !pending.local_confirmed || !pending.remote_confirmed {
            return Ok(());
        }
        let session = pending.session;
        let challenge_nonce = pending.challenge_nonce.clone();
        let sender_committed = pending.sender_committed;
        let receiver_committed = pending.receiver_committed;
        let sender_id = self
            .active_sender
            .as_ref()
            .map(|active| active.sender_id.clone())
            .unwrap_or_default();

        if !sender_committed {
            let approval = PairingApproval {
                magic: PAIRING_APPROVAL_MAGIC,
                challenge_nonce: challenge_nonce.clone(),
                transcript_hash: pairing_transcript_hash(
                    &challenge_nonce,
                    &self.identity.receiver_id,
                    &sender_id,
                    PAIRING_APPROVAL_PHASE,
                ),
            };
            return self.send_control_message(session, &approval);
        }

        if !receiver_committed {
            let now_ms = self.now_ms();
            let active = self.active_sender.as_ref().expect("active sender");
            let previous_trusted = self.trusted.clone();
            self.trusted.upsert(trusted_device_from_pairing(
                &active.sender_id,
                &active.device_name,
                &active.public_key,
                now_ms,
            ));
            if let Err(error) = self.persist_trusted() {
                self.trusted = previous_trusted;
                return Err(error);
            }
            if let Some(pending) = self.pending_pairing.as_mut() {
                pending.receiver_committed = true;
            }
        }

        let complete = PairingComplete {
            magic: PAIRING_COMPLETE_MAGIC,
            challenge_nonce: challenge_nonce.clone(),
            transcript_hash: pairing_transcript_hash(
                &challenge_nonce,
                &self.identity.receiver_id,
                &sender_id,
                PAIRING_COMPLETE_PHASE,
            ),
        };
        self.send_control_message(session, &complete)?;

        if let Some(sender) = self.active_sender.as_mut() {
            sender.video_allowed = true;
        }
        self.pending_pairing = None;
        self.begin_streaming(session)
    }

    fn send_control_message<M: Message>(
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
        let _ = self.decoder.flush();
        if self.transport.is_connected() {
            self.transport
                .close(picoo_transport::SessionId(1), CloseReason::LocalClose);
        }
        self.placeholder_after = None;
        self.status = ReceiverStatus::Disconnected;
        self.active_sender = None;
        self.pending_pairing = None;
        self.last_media_error = None;
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

    /// Test-only decoder injection keeps synthetic payload support outside the
    /// production platform decoder.
    #[cfg(test)]
    pub fn set_decoder_for_test(&mut self, decoder: Box<dyn AccessUnitDecoder>) {
        self.decoder = decoder;
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

    /// Decode H.264 access unit once → FrameHub + Shared Frame Ring.
    fn publish_access_unit(&mut self, access_unit: Bytes) -> Result<(), ReceiverError> {
        self.ingress.access_units += 1;
        self.ingress.decode_invocations += 1;
        let decoded = match self
            .decoder
            .decode_access_unit(&access_unit, self.current_stream_config.as_ref())
        {
            Ok(decoded) => decoded,
            Err(error) => {
                self.stats_reporter.record_decoder_drop();
                self.last_media_error = Some(error.to_string());
                tracing::warn!("H.264 access unit decode failed: {error}");
                return Ok(());
            }
        };
        match decoded {
            Some(frame) => {
                // Prefer StreamConfig.rotation from Sender when present (PUC-005 / MEDIA-009).
                let rotation = self
                    .current_stream_config
                    .as_ref()
                    .map(|c| c.rotation)
                    .unwrap_or(frame.rotation);
                self.publish_nv12_frame(
                    frame.width,
                    frame.height,
                    frame.stride,
                    rotation,
                    frame.timestamp_us,
                    &frame.nv12,
                )?;
                self.ingress.decoded_frames += 1;
                self.last_media_error = None;
            }
            None => {
                self.stats_reporter.record_decoder_drop();
            }
        }
        Ok(())
    }

    fn publish_nv12_frame(
        &mut self,
        width: u32,
        height: u32,
        stride: u32,
        rotation: u32,
        timestamp_us: u64,
        nv12: &[u8],
    ) -> Result<(), ReceiverError> {
        // REQ-PICOO-MEDIA-009: rotate pixels to upright before FrameHub / Shared Ring / VCam.
        // REQ-PICOO-MEDIA-004: then apply remote StreamConfig.mirrored in upright space.
        let rotated_buf =
            picoo_frame_hub::nv12_rotate_clockwise(width, height, stride, rotation, nv12);
        let (width, height, stride, base_pixels): (u32, u32, u32, &[u8]) = match &rotated_buf {
            Some((ow, oh, os, buf)) => (*ow, *oh, *os, buf.as_slice()),
            None => (width, height, stride, nv12),
        };

        let mirrored = self
            .current_stream_config
            .as_ref()
            .is_some_and(|c| c.mirrored);
        let mirrored_owned = if mirrored {
            let mut buf = base_pixels.to_vec();
            picoo_frame_hub::nv12_mirror_horizontal(width, height, stride, &mut buf);
            Some(buf)
        } else {
            None
        };
        let pixels = mirrored_owned.as_deref().unwrap_or(base_pixels);

        // Pixels are upright after rotation; clear metadata so VCam does not double-rotate.
        let published_rotation = 0u32;

        let index = self.frame_hub.begin_write()?;
        self.frame_hub.commit_write(
            index,
            width,
            height,
            stride,
            published_rotation,
            timestamp_us,
            Bytes::copy_from_slice(pixels),
        );
        if let Some(ring) = self.shared_ring.as_mut() {
            ring.publish_nv12(
                width,
                height,
                stride,
                published_rotation,
                timestamp_us,
                pixels,
            )?;
        }
        Ok(())
    }

    pub fn latest_frame(&self) -> Option<&FrameSlot> {
        self.frame_hub.latest_ready()
    }
}

/// Run sender→receiver loopback until one access unit reaches FrameHub.
///
/// Uses the unpaired test bypass — prefer [`run_paired_loopback_access_unit`] for
/// product-path validation (REQ-PICOO-PAIRING-003).
pub fn run_loopback_access_unit(payload: &[u8]) -> Result<Bytes, ReceiverError> {
    use picoo_sender::SenderSession;
    use picoo_transport::{Endpoint, QuicSenderTransport};

    let mut receiver = ReceiverSession::new();
    receiver.decoder = Box::new(picoo_media_decode::StubDecoder::new());
    receiver.set_jitter_target_ms(0);
    receiver.set_permit_unpaired_video(true);
    let bind = receiver.listen(Endpoint {
        host: "127.0.0.1".into(),
        port: 0,
    })?;

    let mut sender = SenderSession::new(QuicSenderTransport::new());
    let endpoint = Endpoint {
        host: bind.ip().to_string(),
        port: bind.port(),
    };
    sender.connect(endpoint)?;

    for _ in 0..500 {
        receiver.pump()?;
        sender.pump()?;
        if sender.is_connected() && receiver.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    if !receiver.is_connected() {
        return Err(ReceiverError::LoopbackTimeout);
    }

    // This helper intentionally exercises the receiver's explicit unpaired test bypass.
    // Production senders never enter Streaming before pairing has committed.
    sender.ingest_and_flush_unchecked_for_test(payload, true, 1, 1)?;

    for _ in 0..200 {
        receiver.pump()?;
        sender.pump().ok();
        if let Some(frame) = receiver.latest_frame() {
            return Ok(frame.pixel_data.clone());
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    Err(ReceiverError::LoopbackTimeout)
}

/// Pairing/session loopback: first-time pairing (short code) then video → FrameHub.
///
/// This explicitly uses `StubDecoder` for arbitrary fixture bytes. It validates
/// the paired transport/session path, not a platform's production H.264 decoder.
/// Does **not** use `permit_unpaired_video` (REQ-PICOO-PAIRING-003).
pub fn run_paired_loopback_access_unit(payload: &[u8]) -> Result<Bytes, ReceiverError> {
    use picoo_sender::SenderSession;
    use picoo_session::SenderStatus;
    use picoo_transport::{Endpoint, QuicSenderTransport};

    let identity = ReceiverIdentity::default();
    let mut receiver = ReceiverSession::new().with_identity(identity.clone());
    receiver.decoder = Box::new(picoo_media_decode::StubDecoder::new());
    receiver.set_jitter_target_ms(0);
    let bind = receiver.listen(Endpoint {
        host: "127.0.0.1".into(),
        port: 0,
    })?;

    let mut sender = SenderSession::new(QuicSenderTransport::new());
    sender.connect(Endpoint {
        host: bind.ip().to_string(),
        port: bind.port(),
    })?;

    for _ in 0..500 {
        receiver.pump()?;
        sender.pump()?;
        if sender.is_connected() && receiver.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    if !receiver.is_connected() {
        return Err(ReceiverError::LoopbackTimeout);
    }

    sender.send_client_hello("loopback-phone", "Loopback Sender", &[0xAAu8, 0xBB, 0xCC])?;

    for _ in 0..200 {
        receiver.pump()?;
        sender.pump()?;
        if receiver.pairing_short_code().is_some() && sender.pairing_short_code().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    if receiver.pairing_short_code().is_none() {
        return Err(ReceiverError::LoopbackTimeout);
    }

    receiver.confirm_pairing_locally()?;
    sender.send_pairing_confirm(&identity.receiver_id)?;

    for _ in 0..200 {
        receiver.pump()?;
        sender.pump()?;
        if receiver.status() == ReceiverStatus::Streaming
            && sender.status() == SenderStatus::Streaming
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    if receiver.status() != ReceiverStatus::Streaming || sender.status() != SenderStatus::Streaming
    {
        return Err(ReceiverError::LoopbackTimeout);
    }

    sender.ingest_and_flush(payload, true, 1, 1)?;

    for _ in 0..200 {
        receiver.pump()?;
        sender.pump().ok();
        if let Some(frame) = receiver.latest_frame() {
            return Ok(frame.pixel_data.clone());
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    Err(ReceiverError::LoopbackTimeout)
}

#[cfg(test)]
mod tests;
