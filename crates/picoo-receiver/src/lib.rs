//! Desktop receiver session: QUIC ingress → reassembly → FrameHub.
//!
//! REQ-PICOO-FRAME-001, REQ-PICOO-MEDIA-005/006 via picoo-media-decode.
//! REQ-PICOO-PAIRING-*: ClientHello/ServerHello gate before video ingress.

pub const DEFAULT_SHARED_RING_NAME: &str = "picoo-camera-v1";

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bytes::Bytes;
use picoo_frame_hub::{
    waiting_placeholder, FrameHub, FrameSlot, SharedFrameRingProducer, PLACEHOLDER_HEIGHT,
    PLACEHOLDER_WIDTH,
};
use picoo_media_decode::{create_platform_decoder, AccessUnitDecoder, DecodeError};
use picoo_packet::ReassemblyMap;
use picoo_pairing::{
    new_pairing_challenge, random_challenge_nonce, trusted_device_from_pairing,
    verify_pairing_confirm, PairingError, PairingHandshakeError, StoreError, TrustedDeviceStore,
};
use picoo_protocol::control::{
    Capabilities, ClientHello, EncoderCommand, PairingChallenge as PairingChallengeMsg,
    PairingConfirm, ReceiverStats as ReceiverStatsMsg, Resolution, ServerHello, StreamConfig,
};
use picoo_protocol::ALPN;
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
}

struct StatsReporter {
    last_sent: Instant,
    window_packets: u64,
    window_bytes: u64,
    last_reassembly_drops: u64,
}

impl StatsReporter {
    fn new() -> Self {
        Self {
            last_sent: Instant::now(),
            window_packets: 0,
            window_bytes: 0,
            last_reassembly_drops: 0,
        }
    }

    fn record_packet(&mut self, payload_len: usize) {
        self.window_packets += 1;
        self.window_bytes += payload_len as u64;
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
    local_pairing_confirmed: bool,
    status: ReceiverStatus,
    ingress: IngressStats,
    stats_reporter: StatsReporter,
    permit_unpaired_video: bool,
    shared_ring: Option<SharedFrameRingProducer>,
    current_stream_config: Option<StreamConfig>,
    receiver_capabilities_sent: Option<()>,
    decoder: Box<dyn AccessUnitDecoder>,
    /// After peer disconnect, keep last frame this long before placeholder (REQ-PICOO-FRAME-005).
    last_frame_hold: Duration,
    placeholder_after: Option<Instant>,
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
            reassembly: ReassemblyMap::new(8, 16),
            frame_hub: FrameHub::new(),
            identity: ReceiverIdentity::default(),
            trusted: TrustedDeviceStore::new(),
            trusted_store_path: None,
            active_sender: None,
            pending_pairing: None,
            local_pairing_confirmed: false,
            status: ReceiverStatus::Disconnected,
            ingress: IngressStats::default(),
            stats_reporter: StatsReporter::new(),
            permit_unpaired_video: false,
            shared_ring: None,
            current_stream_config: None,
            receiver_capabilities_sent: None,
            decoder: create_platform_decoder(),
            last_frame_hold: Duration::from_millis(500),
            placeholder_after: None,
        }
    }

    /// Attach a cross-process Shared Frame Ring for VCam consumption (REQ-PICOO-FRAME-003).
    pub fn attach_shared_ring(&mut self, name: &str) -> Result<(), ReceiverError> {
        let ring = SharedFrameRingProducer::open_or_create(
            name,
            picoo_frame_hub::DEFAULT_MAX_FRAME_BYTES,
        )?;
        self.shared_ring = Some(ring);
        self.publish_waiting_placeholder()?;
        Ok(())
    }

    pub fn publish_waiting_placeholder(&mut self) -> Result<(), ReceiverError> {
        let nv12 = waiting_placeholder();
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

    pub fn status(&self) -> ReceiverStatus {
        self.status
    }

    pub fn ingress_stats(&self) -> IngressStats {
        self.ingress
    }

    /// Backward-compatible alias for ingress counters.
    pub fn stats(&self) -> IngressStats {
        self.ingress
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

    /// User confirmed the six-digit code on desktop (PUC-001).
    pub fn confirm_pairing_locally(&mut self) {
        self.local_pairing_confirmed = true;
    }

    pub fn is_awaiting_pairing_confirm(&self) -> bool {
        self.pending_pairing.is_some()
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
        self.transport.pump()?;

        while let Some(event) = self.transport.poll_event() {
            match event {
                TransportEvent::Connected(_) => {
                    self.placeholder_after = None;
                    self.status = ReceiverStatus::Connecting;
                }
                TransportEvent::Disconnected(_, _) => {
                    self.on_peer_disconnected();
                }
                TransportEvent::ControlMessage(session, msg) => {
                    self.handle_control(session, msg)?;
                }
                TransportEvent::VideoPacket(session, packet) => {
                    self.ingress.packets_received += 1;
                    if !self.video_allowed() {
                        self.ingress.packets_dropped_unpaired += 1;
                        continue;
                    }
                    self.stats_reporter.record_packet(packet.payload.len());
                    if let Some(access_unit) = self.reassembly.ingest(packet).ok().flatten() {
                        self.publish_access_unit(access_unit)?;
                    }
                    if self.reassembly.take_keyframe_loss() {
                        self.send_request_keyframe(session)?;
                    }
                }
            }
        }

        self.maybe_finalize_disconnect_hold()?;
        self.maybe_send_receiver_stats()?;

        Ok(())
    }

    fn on_peer_disconnected(&mut self) {
        let had_live_frame = self.status == ReceiverStatus::Streaming
            && self.frame_hub.latest_ready().is_some();
        self.active_sender = None;
        self.pending_pairing = None;
        self.local_pairing_confirmed = false;
        self.reassembly = ReassemblyMap::new(8, 16);
        self.current_stream_config = None;
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
    }

    fn maybe_finalize_disconnect_hold(&mut self) -> Result<(), ReceiverError> {
        let Some(deadline) = self.placeholder_after else {
            return Ok(());
        };
        if Instant::now() < deadline {
            return Ok(());
        }
        self.placeholder_after = None;
        self.publish_waiting_placeholder()?;
        self.status = if self.bind_addr().is_some() {
            ReceiverStatus::Discovering
        } else {
            ReceiverStatus::Disconnected
        };
        Ok(())
    }

    fn maybe_send_receiver_stats(&mut self) -> Result<(), ReceiverError> {
        if self.status != ReceiverStatus::Streaming {
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

        let stats = ReceiverStatsMsg {
            rtt_ms: 0.0,
            packet_loss: 0.0,
            jitter_ms: 0.0,
            reassembly_drop,
            decoder_drop: 0,
            frame_age_ms,
            receive_bitrate,
            jitter_buffer_depth_ms: 0.0,
        };

        self.send_control_message(session, &stats)?;
        self.transport.pump()?;

        self.stats_reporter.last_sent = Instant::now();
        self.stats_reporter.window_packets = 0;
        self.stats_reporter.window_bytes = 0;
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
            // Prost will decode many unrelated blobs as PairingConfirm with an empty
            // signature — only accept SHA-256-length confirm signatures.
            if let Ok(confirm) = PairingConfirm::decode(msg.as_ref()) {
                if confirm.confirm_signature.len() == 32 {
                    return self.handle_pairing_confirm(session, msg);
                }
            }
            return Ok(());
        }
        if self.active_sender.is_none() {
            return self.handle_client_hello(session, msg);
        }
        if let Ok(config) = StreamConfig::decode(msg.as_ref()) {
            return self.handle_stream_config(session, config);
        }
        Ok(())
    }

    fn handle_stream_config(
        &mut self,
        session: SessionId,
        config: StreamConfig,
    ) -> Result<(), ReceiverError> {
        self.current_stream_config = Some(config);
        if self.receiver_capabilities_sent.is_none() {
            self.send_capabilities(session)?;
            self.receiver_capabilities_sent = Some(());
        }
        Ok(())
    }

    fn send_capabilities(&mut self, session: SessionId) -> Result<(), ReceiverError> {
        let capabilities = Capabilities {
            codecs: vec!["h264".into()],
            resolutions: vec![
                Resolution {
                    width: 1280,
                    height: 720,
                },
                Resolution {
                    width: 1920,
                    height: 1080,
                },
            ],
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

    fn begin_streaming(&mut self, _session: SessionId) -> Result<(), ReceiverError> {
        self.status = ReceiverStatus::Streaming;
        Ok(())
    }

    fn handle_client_hello(&mut self, session: SessionId, msg: Bytes) -> Result<(), ReceiverError> {
        let hello = ClientHello::decode(msg.as_ref())
            .map_err(|e| ReceiverError::Protocol(format!("ClientHello decode: {e}")))?;

        let paired = self
            .trusted
            .verify_paired_key(&hello.sender_id, &hello.public_key)
            .is_ok();

        let server_hello = ServerHello {
            receiver_id: self.identity.receiver_id.clone(),
            display_name: self.identity.display_name.clone(),
            protocol_version: ALPN.into(),
            public_key: self.identity.public_key.clone(),
            pairing_required: !paired,
        };
        self.send_control_message(session, &server_hello)?;

        if paired {
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
        });
        self.local_pairing_confirmed = false;
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

        if session != pending.session {
            return Err(ReceiverError::Protocol("pairing session mismatch".into()));
        }

        if !self.local_pairing_confirmed {
            return Err(ReceiverError::Protocol(
                "desktop pairing not confirmed locally".into(),
            ));
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

        let now_ms = self.now_ms();

        let active = self.active_sender.as_ref().expect("active sender");
        self.trusted.upsert(trusted_device_from_pairing(
            &active.sender_id,
            &active.device_name,
            &active.public_key,
            now_ms,
        ));
        self.persist_trusted()?;

        if let Some(sender) = self.active_sender.as_mut() {
            sender.video_allowed = true;
        }
        self.pending_pairing = None;
        self.local_pairing_confirmed = false;
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
        if self.transport.is_connected() {
            self.transport
                .close(picoo_transport::SessionId(1), CloseReason::LocalClose);
        }
        self.placeholder_after = None;
        self.status = ReceiverStatus::Disconnected;
        self.active_sender = None;
        self.pending_pairing = None;
        self.local_pairing_confirmed = false;
        let _ = self.publish_waiting_placeholder();
    }

    /// Test-only: shorten/extend last-frame hold before placeholder (REQ-PICOO-FRAME-005).
    #[cfg(test)]
    pub fn set_last_frame_hold_for_test(&mut self, hold: Duration) {
        self.last_frame_hold = hold;
    }

    /// Test-only: simulate peer disconnect without waiting on QUIC teardown.
    #[cfg(test)]
    pub fn inject_peer_disconnect_for_test(&mut self) {
        self.on_peer_disconnected();
    }

    /// Decode H.264 access unit once → FrameHub + Shared Frame Ring.
    fn publish_access_unit(&mut self, access_unit: Bytes) -> Result<(), ReceiverError> {
        self.ingress.access_units += 1;
        if let Some(frame) = self
            .decoder
            .decode_access_unit(&access_unit, self.current_stream_config.as_ref())?
        {
            self.publish_nv12_frame(
                frame.width,
                frame.height,
                frame.stride,
                frame.rotation,
                frame.timestamp_us,
                &frame.nv12,
            )?;
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
        let index = self.frame_hub.begin_write()?;
        self.frame_hub.commit_write(
            index,
            width,
            height,
            stride,
            rotation,
            timestamp_us,
            Bytes::copy_from_slice(nv12),
        );
        if let Some(ring) = self.shared_ring.as_mut() {
            ring.publish_nv12(width, height, stride, rotation, timestamp_us, nv12)?;
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

/// Full product-path loopback: first-time pairing (short code) then video → FrameHub.
///
/// Does **not** use `permit_unpaired_video` (REQ-PICOO-PAIRING-003).
pub fn run_paired_loopback_access_unit(payload: &[u8]) -> Result<Bytes, ReceiverError> {
    use picoo_sender::SenderSession;
    use picoo_transport::{Endpoint, QuicSenderTransport};

    let identity = ReceiverIdentity::default();
    let mut receiver = ReceiverSession::new().with_identity(identity.clone());
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

    receiver.confirm_pairing_locally();
    sender.send_pairing_confirm(&identity.receiver_id)?;

    for _ in 0..200 {
        receiver.pump()?;
        sender.pump()?;
        if receiver.status() == ReceiverStatus::Streaming {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    if receiver.status() != ReceiverStatus::Streaming {
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
