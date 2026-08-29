//! Sender session: packetization + transport flush + reconnect + bitrate control.
//!
//! REQ-PICOO-SESSION-001, REQ-PICOO-TRANSPORT-004, REQ-PICOO-MEDIA-007

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use picoo_metrics::ReceiverStats as MetricsReceiverStats;
use picoo_pairing::{pairing_confirm_signature, trusted_device_from_pairing, TrustedDeviceStore};
use picoo_protocol::control::{
    camera_command, encoder_command, CameraCommand, Capabilities, ClientHello, EncoderCommand,
    PairingChallenge, PairingConfirm, ReceiverStats as ReceiverStatsMsg, ServerHello, SessionError,
    StartStream, StopStream,
};
use picoo_protocol::VideoPacket;
use picoo_protocol::ALPN;
use picoo_rate_control::{BitrateAction, BitrateController};
use picoo_session::{ReconnectBackoff, SenderStatus};
use picoo_transport::{Endpoint, PicooTransport, SessionId, TransportEvent};
use prost::Message;

use crate::stream_config::StreamConfigParams;
use crate::{SenderError, SenderPipeline, SenderStats};

const DEFAULT_INITIAL_BITRATE_BPS: u32 = 6_000_000;
const DEFAULT_MIN_BITRATE_BPS: u32 = 3_000_000;
const DEFAULT_MAX_BITRATE_BPS: u32 = 10_000_000;

#[derive(Debug, Clone)]
struct SenderPairing {
    receiver_id: String,
    display_name: String,
    public_key: Vec<u8>,
    challenge_nonce: Vec<u8>,
    short_code: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionStats {
    pub pipeline: SenderStats,
    pub sent_datagrams: u64,
}

pub struct SenderSession<T: PicooTransport> {
    pipeline: SenderPipeline,
    transport: T,
    session: Option<SessionId>,
    sent_datagrams: u64,
    pairing: Option<SenderPairing>,
    sender_id: Option<String>,
    hello_params: Option<(String, String, Vec<u8>)>,
    trusted: TrustedDeviceStore,
    trusted_store_path: Option<PathBuf>,
    status: SenderStatus,
    last_endpoint: Option<Endpoint>,
    reconnect_backoff: ReconnectBackoff,
    reconnect_after: Option<Instant>,
    /// Last delay chosen by [`Self::schedule_reconnect`] (TRANSPORT-004 observability).
    last_scheduled_reconnect_delay_ms: Option<u64>,
    auto_reconnect: bool,
    bitrate: BitrateController,
    last_bitrate_action: BitrateAction,
    last_receiver_stats: Option<MetricsReceiverStats>,
    pending_stream_config: Option<StreamConfigParams>,
    receiver_capabilities: Option<Capabilities>,
    stream_config_sent: bool,
    /// Receiver asked for IDR via EncoderCommand (REQ-PICOO-SESSION-003/004).
    keyframe_requested: bool,
    /// ABR last rung: host should drop capture height (typically 1080→720).
    resolution_downshift_requested: bool,
    /// ABR recovery: host may restore preferred height (typically 720→1080).
    resolution_upshift_requested: bool,
    /// Latest CameraCommand from receiver (PUC-005 desktop remote control).
    pending_camera_command: Option<CameraCommand>,
    /// Last SessionError code from receiver (e.g. PUBLIC_KEY_CHANGED).
    last_session_error: Option<String>,
}

impl<T: PicooTransport> SenderSession<T> {
    pub fn new(transport: T) -> Self {
        Self {
            pipeline: SenderPipeline::default(),
            transport,
            session: None,
            sent_datagrams: 0,
            pairing: None,
            sender_id: None,
            hello_params: None,
            trusted: TrustedDeviceStore::new(),
            trusted_store_path: None,
            status: SenderStatus::Disconnected,
            last_endpoint: None,
            reconnect_backoff: ReconnectBackoff::default(),
            reconnect_after: None,
            last_scheduled_reconnect_delay_ms: None,
            auto_reconnect: true,
            bitrate: BitrateController::new(
                DEFAULT_INITIAL_BITRATE_BPS,
                DEFAULT_MIN_BITRATE_BPS,
                DEFAULT_MAX_BITRATE_BPS,
            ),
            last_bitrate_action: BitrateAction::Hold,
            last_receiver_stats: None,
            pending_stream_config: Some(StreamConfigParams::default()),
            receiver_capabilities: None,
            stream_config_sent: false,
            keyframe_requested: false,
            resolution_downshift_requested: false,
            resolution_upshift_requested: false,
            pending_camera_command: None,
            last_session_error: None,
        }
    }

    pub fn status(&self) -> SenderStatus {
        self.status
    }

    /// Access the underlying transport (loss injection / diagnostics in tests).
    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    /// Surface camera/mic permission gate to UI (REQ-PICOO-SESSION-001).
    pub fn mark_permission_required(&mut self) {
        self.status = SenderStatus::PermissionRequired;
    }

    /// Clear permission gate once the host grants access (REQ-PICOO-SESSION-001).
    pub fn clear_permission_required(&mut self) {
        if self.status == SenderStatus::PermissionRequired {
            self.status = SenderStatus::Disconnected;
        }
    }

    pub fn set_auto_reconnect(&mut self, enabled: bool) {
        self.auto_reconnect = enabled;
    }

    /// Delay scheduled by the most recent reconnect arming (REQ-PICOO-TRANSPORT-004).
    pub fn last_scheduled_reconnect_delay_ms(&self) -> Option<u64> {
        self.last_scheduled_reconnect_delay_ms
    }

    /// 1-based reconnect attempt while in [`SenderStatus::Reconnecting`].
    pub fn reconnect_attempt(&self) -> u32 {
        if self.status == SenderStatus::Reconnecting {
            self.reconnect_backoff.attempt()
        } else {
            0
        }
    }

    /// Active ABR ladder height after downshift/upshift acknowledgements.
    pub fn bitrate_active_height(&self) -> u32 {
        self.bitrate.active_height()
    }

    pub fn current_bitrate_bps(&self) -> u32 {
        self.bitrate.current_bitrate_bps()
    }

    pub fn last_bitrate_action(&self) -> BitrateAction {
        self.last_bitrate_action
    }

    pub fn last_receiver_stats(&self) -> Option<&MetricsReceiverStats> {
        self.last_receiver_stats.as_ref()
    }

    pub fn receiver_capabilities(&self) -> Option<&Capabilities> {
        self.receiver_capabilities.as_ref()
    }

    pub fn set_stream_config(&mut self, config: StreamConfigParams) {
        // Allow re-send when SPS/PPS arrive late or resolution/mirror changes (PUC-005/006).
        self.pending_stream_config = Some(config);
        self.stream_config_sent = false;
        self.apply_capability_height_clamp();
    }

    /// Max height from receiver Capabilities (0 if unknown). REQ-PICOO-MEDIA-002.
    pub fn receiver_max_height(&self) -> u32 {
        self.receiver_capabilities
            .as_ref()
            .map(|caps| caps.resolutions.iter().map(|r| r.height).max().unwrap_or(0))
            .unwrap_or(0)
    }

    /// Clamp pending StreamConfig to advertised Capabilities heights.
    fn apply_capability_height_clamp(&mut self) {
        let max_h = self.receiver_max_height();
        if max_h == 0 {
            return;
        }
        let Some(cfg) = self.pending_stream_config.as_mut() else {
            return;
        };
        if cfg.height <= max_h {
            return;
        }
        // Map onto the receiver ladder (480p / 720p / 1080p).
        if max_h < 720 {
            cfg.width = 854;
            cfg.height = 480;
        } else if max_h < 1080 {
            cfg.width = 1280;
            cfg.height = 720;
        } else {
            cfg.width = 1920;
            cfg.height = 1080.min(max_h);
        }
        self.stream_config_sent = false;
        self.bitrate.sync_encode_height(cfg.height);
    }

    /// User / capability preferred capture height (does not change active encode height).
    pub fn set_preferred_height(&mut self, height: u32) {
        self.bitrate.set_preferred_height(height);
    }

    /// Host thermal policy — block ABR upshift while overheating (MEDIA-010).
    pub fn set_thermal_hold(&mut self, hold: bool) {
        self.bitrate.set_thermal_hold(hold);
    }

    pub fn thermal_hold(&self) -> bool {
        self.bitrate.thermal_hold()
    }

    /// Host applied encode height (thermal / user / ABR). Syncs ABR ladder.
    pub fn sync_encode_height(&mut self, height: u32) {
        self.bitrate.sync_encode_height(height);
    }

    pub fn stream_config_sent(&self) -> bool {
        self.stream_config_sent
    }

    pub fn pending_stream_config(&self) -> Option<&StreamConfigParams> {
        self.pending_stream_config.as_ref()
    }

    /// Consume a pending IDR request from the receiver (REQ-PICOO-SESSION-003).
    pub fn take_keyframe_request(&mut self) -> bool {
        let pending = self.keyframe_requested;
        self.keyframe_requested = false;
        pending
    }

    /// Consume a desktop-originated CameraCommand (PUC-005).
    pub fn take_camera_command(&mut self) -> Option<CameraCommand> {
        self.pending_camera_command.take()
    }

    pub fn last_session_error(&self) -> Option<&str> {
        self.last_session_error.as_deref()
    }

    /// Sender → Receiver StartStream (PAIRING-003 / PROTOCOL control plane).
    pub fn send_start_stream(&mut self) -> Result<(), SenderError> {
        let session = self.session.ok_or(SenderError::NotConnected)?;
        let msg = StartStream { magic: 1 };
        let mut buf = Vec::new();
        msg.encode(&mut buf)
            .map_err(|e| SenderError::Protocol(e.to_string()))?;
        self.transport
            .send_control(session, bytes::Bytes::from(buf))
            .map_err(SenderError::Transport)?;
        self.drain_events();
        Ok(())
    }

    /// Sender → Receiver StopStream.
    pub fn send_stop_stream(&mut self) -> Result<(), SenderError> {
        let session = self.session.ok_or(SenderError::NotConnected)?;
        let msg = StopStream { magic: 2 };
        let mut buf = Vec::new();
        msg.encode(&mut buf)
            .map_err(|e| SenderError::Protocol(e.to_string()))?;
        self.transport
            .send_control(session, bytes::Bytes::from(buf))
            .map_err(SenderError::Transport)?;
        self.drain_events();
        Ok(())
    }

    /// Consume ABR resolution downshift hint (REQ-PICOO-MEDIA-010 / PUC-006).
    pub fn take_resolution_downshift(&mut self) -> bool {
        let pending = self.resolution_downshift_requested;
        self.resolution_downshift_requested = false;
        if pending {
            self.bitrate.acknowledge_resolution_downshift();
        }
        pending
    }

    /// Consume ABR resolution upshift hint (REQ-PICOO-MEDIA-010 / PUC-006).
    pub fn take_resolution_upshift(&mut self) -> bool {
        let pending = self.resolution_upshift_requested;
        self.resolution_upshift_requested = false;
        if pending {
            self.bitrate.acknowledge_resolution_upshift();
        }
        pending
    }

    pub fn with_trusted_store(mut self, path: impl AsRef<Path>) -> Result<Self, SenderError> {
        let path = path.as_ref().to_path_buf();
        self.trusted = TrustedDeviceStore::load_from_path(&path)?;
        self.trusted_store_path = Some(path);
        Ok(self)
    }

    pub fn attach_trusted_store(&mut self, path: impl AsRef<Path>) -> Result<(), SenderError> {
        let path = path.as_ref().to_path_buf();
        self.trusted = TrustedDeviceStore::load_from_path(&path)?;
        self.trusted_store_path = Some(path);
        Ok(())
    }

    pub fn trusted_devices(&self) -> &TrustedDeviceStore {
        &self.trusted
    }

    pub fn remove_trusted_device(&mut self, device_id: &str) -> Result<bool, SenderError> {
        let removed = self.trusted.remove(device_id);
        if removed {
            self.persist_trusted()?;
        }
        Ok(removed)
    }

    pub fn connected_receiver_id(&self) -> Option<&str> {
        self.pairing
            .as_ref()
            .and_then(|p| (!p.receiver_id.is_empty()).then_some(p.receiver_id.as_str()))
    }

    /// Display name from ServerHello (empty until hello arrives).
    pub fn connected_receiver_display_name(&self) -> Option<&str> {
        self.pairing
            .as_ref()
            .and_then(|p| (!p.display_name.is_empty()).then_some(p.display_name.as_str()))
    }

    fn persist_trusted(&self) -> Result<(), SenderError> {
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

    fn enter_streaming(&mut self) {
        self.status = SenderStatus::Streaming;
        // Fresh streaming (including post-reconnect) needs an IDR (REQ-PICOO-SESSION-004).
        self.keyframe_requested = true;
        let _ = self.send_pending_stream_config();
    }

    fn send_pending_stream_config(&mut self) -> Result<(), SenderError> {
        if self.stream_config_sent {
            return Ok(());
        }
        let Some(config) = self.pending_stream_config.clone() else {
            return Ok(());
        };
        self.send_stream_config(&config)?;
        self.stream_config_sent = true;
        Ok(())
    }

    pub fn send_stream_config(&mut self, config: &StreamConfigParams) -> Result<(), SenderError> {
        let session = self.session.ok_or(SenderError::NotConnected)?;
        let msg = config.to_proto();
        let mut buf = Vec::new();
        msg.encode(&mut buf)
            .map_err(|e| SenderError::Protocol(e.to_string()))?;
        self.transport
            .send_control(session, bytes::Bytes::from(buf))
            .map_err(SenderError::Transport)?;
        self.pending_stream_config = Some(config.clone());
        Ok(())
    }

    fn schedule_reconnect(&mut self) {
        if !self.auto_reconnect || self.last_endpoint.is_none() {
            self.status = SenderStatus::Disconnected;
            return;
        }
        let delay_ms = self.reconnect_backoff.next_delay_ms();
        self.last_scheduled_reconnect_delay_ms = Some(delay_ms);
        self.reconnect_after = Some(Instant::now() + Duration::from_millis(delay_ms));
        self.status = SenderStatus::Reconnecting;
    }

    fn try_reconnect(&mut self) -> Result<(), SenderError> {
        let Some(deadline) = self.reconnect_after else {
            return Ok(());
        };
        if Instant::now() < deadline {
            return Ok(());
        }
        self.reconnect_after = None;
        let endpoint = self
            .last_endpoint
            .clone()
            .ok_or(SenderError::NotConnected)?;
        let _ = self.connect(endpoint)?;
        Ok(())
    }

    fn on_connected(&mut self) {
        self.reconnect_backoff.reset();
        self.reconnect_after = None;
        self.status = SenderStatus::Connecting;
        if let Some((sender_id, device_name, public_key)) = self.hello_params.clone() {
            if self
                .send_client_hello(&sender_id, &device_name, &public_key)
                .is_ok()
            {
                self.status = SenderStatus::Negotiating;
            }
        }
    }

    fn drain_events(&mut self) {
        while let Some(event) = self.transport.poll_event() {
            match event {
                TransportEvent::Connected(session) => {
                    self.session = Some(session);
                    self.on_connected();
                }
                TransportEvent::ControlMessage(_, msg) => self.handle_control(msg),
                TransportEvent::Disconnected(_, _) => {
                    self.session = None;
                    self.pairing = None;
                    self.stream_config_sent = false;
                    self.pipeline.clear_pending_packets();
                    self.schedule_reconnect();
                }
                TransportEvent::VideoPacket(_, _) => {}
            }
        }
    }

    fn handle_control(&mut self, msg: bytes::Bytes) {
        if let Ok(stats) = ReceiverStatsMsg::decode(msg.as_ref()) {
            let metrics = MetricsReceiverStats {
                rtt_ms: stats.rtt_ms,
                packet_loss: stats.packet_loss,
                jitter_ms: stats.jitter_ms,
                reassembly_drop: stats.reassembly_drop,
                decoder_drop: stats.decoder_drop,
                frame_age_ms: stats.frame_age_ms,
                receive_bitrate: stats.receive_bitrate,
                jitter_buffer_depth_ms: stats.jitter_buffer_depth_ms,
            };
            self.last_receiver_stats = Some(metrics.clone());
            self.last_bitrate_action = self.bitrate.update(&metrics);
            if self.last_bitrate_action == BitrateAction::DownshiftResolution {
                self.resolution_downshift_requested = true;
            }
            if self.last_bitrate_action == BitrateAction::UpshiftResolution {
                self.resolution_upshift_requested = true;
            }
            // REQ-PICOO-SESSION-001: Network Unstable mirrors ARCH loss thresholds.
            if matches!(
                self.status,
                SenderStatus::Streaming | SenderStatus::NetworkUnstable
            ) {
                if metrics.packet_loss > 0.03 {
                    self.status = SenderStatus::NetworkUnstable;
                } else if metrics.packet_loss < 0.01 {
                    self.status = SenderStatus::Streaming;
                }
            }
            return;
        }
        if let Ok(command) = EncoderCommand::decode(msg.as_ref()) {
            if command.command == encoder_command::Command::RequestKeyframe as i32 {
                self.keyframe_requested = true;
                return;
            }
        }
        if let Ok(cam) = CameraCommand::decode(msg.as_ref()) {
            if cam.command != camera_command::Command::Unspecified as i32 {
                self.pending_camera_command = Some(cam);
                return;
            }
        }
        if let Ok(capabilities) = Capabilities::decode(msg.as_ref()) {
            // Empty Capabilities is a prost false-positive for almost any blob.
            if !capabilities.codecs.is_empty() {
                self.receiver_capabilities = Some(capabilities);
                self.apply_capability_height_clamp();
                if self.status == SenderStatus::Negotiating {
                    self.enter_streaming();
                }
                return;
            }
        }
        if let Ok(challenge) = PairingChallenge::decode(msg.as_ref()) {
            let valid = challenge.challenge_nonce.len() == 32
                && challenge.short_code.len() == 6
                && challenge.short_code.chars().all(|c| c.is_ascii_digit());
            if valid {
                if let Some(pairing) = self.pairing.as_mut() {
                    pairing.challenge_nonce = challenge.challenge_nonce;
                    pairing.short_code = challenge.short_code;
                } else {
                    self.pairing = Some(SenderPairing {
                        receiver_id: String::new(),
                        display_name: String::new(),
                        public_key: Vec::new(),
                        challenge_nonce: challenge.challenge_nonce,
                        short_code: challenge.short_code,
                    });
                }
                self.status = SenderStatus::Pairing;
                return;
            }
        }
        // Known SessionError codes before ServerHello — both use string field 1.
        if let Ok(err) = SessionError::decode(msg.as_ref()) {
            if matches!(err.code.as_str(), "UNPAIRED" | "PUBLIC_KEY_CHANGED") {
                self.last_session_error = Some(err.code);
                return;
            }
        }
        if let Ok(hello) = ServerHello::decode(msg.as_ref()) {
            // Real Hello needs non-empty id + PCP version (empty ver = false positive).
            if hello.receiver_id.is_empty() || hello.protocol_version.is_empty() {
                return;
            }
            // ARCH-PICOO-PROTOCOL-001: reject mismatched PCP version fail-fast.
            if hello.protocol_version != picoo_protocol::ALPN {
                if let Some(session) = self.session.take() {
                    self.transport
                        .close(session, picoo_transport::CloseReason::LocalClose);
                }
                self.status = SenderStatus::Disconnected;
                self.pairing = None;
                return;
            }
            if self.trusted.is_paired(&hello.receiver_id) {
                if self
                    .trusted
                    .verify_paired_key(&hello.receiver_id, &hello.public_key)
                    .is_err()
                {
                    if let Some(session) = self.session.take() {
                        self.transport
                            .close(session, picoo_transport::CloseReason::LocalClose);
                    }
                    self.status = SenderStatus::Disconnected;
                    self.pairing = None;
                    return;
                }
                self.trusted
                    .touch_last_connected(&hello.receiver_id, self.now_ms());
                let _ = self.persist_trusted();
            }

            if hello.pairing_required {
                if let Some(pairing) = self.pairing.as_mut() {
                    pairing.receiver_id = hello.receiver_id;
                    pairing.display_name = hello.display_name;
                    pairing.public_key = hello.public_key;
                } else {
                    self.pairing = Some(SenderPairing {
                        receiver_id: hello.receiver_id,
                        display_name: hello.display_name,
                        public_key: hello.public_key,
                        challenge_nonce: Vec::new(),
                        short_code: String::new(),
                    });
                }
                self.status = SenderStatus::Pairing;
            } else {
                if let Some(pairing) = self.pairing.as_mut() {
                    pairing.receiver_id = hello.receiver_id;
                    pairing.display_name = hello.display_name;
                    pairing.public_key = hello.public_key;
                } else {
                    self.pairing = Some(SenderPairing {
                        receiver_id: hello.receiver_id,
                        display_name: hello.display_name,
                        public_key: hello.public_key,
                        challenge_nonce: Vec::new(),
                        short_code: String::new(),
                    });
                }
                self.enter_streaming();
            }
        }
    }

    pub fn stats(&self) -> SessionStats {
        SessionStats {
            pipeline: self.pipeline.stats(),
            sent_datagrams: self.sent_datagrams,
        }
    }

    pub fn is_connected(&self) -> bool {
        self.session.is_some()
    }

    pub fn connect(&mut self, endpoint: Endpoint) -> Result<SessionId, SenderError> {
        // Explicit connect re-enables automatic recovery after a user disconnect.
        self.pipeline.clear_pending_packets();
        self.auto_reconnect = true;
        self.reconnect_after = None;
        self.last_endpoint = Some(endpoint.clone());
        self.status = SenderStatus::Connecting;
        let session = self
            .transport
            .connect(endpoint)
            .map_err(SenderError::Transport)?;
        self.drain_events();
        Ok(session)
    }

    /// User-initiated stop: do not enter Reconnecting (PUC-005 live control).
    pub fn disconnect(&mut self) {
        self.auto_reconnect = false;
        self.reconnect_after = None;
        self.last_endpoint = None;
        if let Some(session) = self.session.take() {
            self.transport
                .close(session, picoo_transport::CloseReason::LocalClose);
        }
        // Drain local Disconnected without scheduling reconnect.
        self.drain_events();
        self.session = None;
        self.pairing = None;
        self.stream_config_sent = false;
        self.pipeline.clear_pending_packets();
        self.status = SenderStatus::Disconnected;
    }

    pub fn pump(&mut self) -> Result<(), SenderError> {
        self.drain_events();
        if self.status == SenderStatus::Reconnecting {
            self.try_reconnect()?;
            self.drain_events();
        }
        if matches!(
            self.status,
            SenderStatus::Streaming | SenderStatus::NetworkUnstable
        ) {
            let _ = self.send_pending_stream_config();
        }
        Ok(())
    }

    pub fn pairing_short_code(&self) -> Option<&str> {
        self.pairing
            .as_ref()
            .and_then(|p| (!p.short_code.is_empty()).then_some(p.short_code.as_str()))
    }

    pub fn ingest_access_unit(
        &mut self,
        data: &[u8],
        is_keyframe: bool,
        pts_us: u64,
        stream_epoch: u32,
    ) -> Result<usize, SenderError> {
        if self.session.is_none() {
            return Err(SenderError::NotConnected);
        }
        self.pipeline
            .ingest_access_unit(data, is_keyframe, pts_us, stream_epoch)
    }

    /// Send all pending VideoPackets over QUIC datagrams.
    pub fn flush_pending(&mut self) -> Result<usize, SenderError> {
        let session = self.session.ok_or(SenderError::NotConnected)?;
        let packets: Vec<VideoPacket> = self.pipeline.take_pending_packets();
        let mut sent = 0usize;
        for packet in packets {
            self.transport
                .send_video(session, packet)
                .map_err(SenderError::Transport)?;
            sent += 1;
        }
        self.sent_datagrams += sent as u64;
        Ok(sent)
    }

    pub fn ingest_and_flush(
        &mut self,
        data: &[u8],
        is_keyframe: bool,
        pts_us: u64,
        stream_epoch: u32,
    ) -> Result<usize, SenderError> {
        self.ingest_access_unit(data, is_keyframe, pts_us, stream_epoch)?;
        self.flush_pending()
    }

    pub fn pending_packets(&self) -> usize {
        self.pipeline.pending_packets().len()
    }

    pub fn send_client_hello(
        &mut self,
        sender_id: &str,
        device_name: &str,
        public_key: &[u8],
    ) -> Result<(), SenderError> {
        self.send_client_hello_with_version(sender_id, device_name, public_key, ALPN)
    }

    /// Emit ClientHello with an explicit protocol_version (protocol fail-fast tests).
    pub fn send_client_hello_with_version(
        &mut self,
        sender_id: &str,
        device_name: &str,
        public_key: &[u8],
        protocol_version: &str,
    ) -> Result<(), SenderError> {
        let session = self.session.ok_or(SenderError::NotConnected)?;
        let hello = ClientHello {
            sender_id: sender_id.into(),
            device_name: device_name.into(),
            protocol_version: protocol_version.into(),
            public_key: public_key.to_vec(),
        };
        self.sender_id = Some(sender_id.into());
        self.hello_params = Some((
            sender_id.to_string(),
            device_name.to_string(),
            public_key.to_vec(),
        ));
        let mut buf = Vec::new();
        hello
            .encode(&mut buf)
            .map_err(|e| SenderError::Protocol(e.to_string()))?;
        self.transport
            .send_control(session, bytes::Bytes::from(buf))
            .map_err(SenderError::Transport)?;
        self.drain_events();
        Ok(())
    }

    pub fn send_pairing_confirm(&mut self, receiver_id: &str) -> Result<(), SenderError> {
        let session = self.session.ok_or(SenderError::NotConnected)?;
        let pairing = self
            .pairing
            .as_ref()
            .ok_or_else(|| SenderError::Protocol("no pairing challenge".into()))?;
        let sender_id = self
            .sender_id
            .as_deref()
            .ok_or_else(|| SenderError::Protocol("missing sender id".into()))?;

        let confirm = PairingConfirm {
            confirm_signature: pairing_confirm_signature(
                &pairing.challenge_nonce,
                receiver_id,
                sender_id,
            ),
        };
        let mut buf = Vec::new();
        confirm
            .encode(&mut buf)
            .map_err(|e| SenderError::Protocol(e.to_string()))?;
        self.transport
            .send_control(session, bytes::Bytes::from(buf))
            .map_err(SenderError::Transport)?;
        if !receiver_id.is_empty() {
            let display_name = if pairing.display_name.is_empty() {
                receiver_id
            } else {
                pairing.display_name.as_str()
            };
            let now_ms = self.now_ms();
            self.trusted.upsert(trusted_device_from_pairing(
                receiver_id,
                display_name,
                if pairing.public_key.is_empty() {
                    &[]
                } else {
                    &pairing.public_key
                },
                now_ms,
            ));
            self.persist_trusted()?;
        }

        // REQ-PICOO-SESSION-004: pairing → streaming must request IDR (same as reconnect).
        self.enter_streaming();
        Ok(())
    }

    /// Inject a decoded control message (tests / ABR loopback harnesses).
    pub fn inject_control_for_test(&mut self, msg: bytes::Bytes) -> Result<(), SenderError> {
        self.handle_control(msg);
        Ok(())
    }

    pub fn force_status_for_test(&mut self, status: SenderStatus) {
        self.status = status;
    }

    /// Close the active transport session (used by reconnect / recovery tests across crates).
    pub fn disconnect_for_test(&mut self, reason: picoo_transport::CloseReason) {
        if let Some(session) = self.session {
            self.transport.close(session, reason);
        }
    }

    /// Simulate a failed reconnect attempt: advance backoff without a successful connect.
    #[cfg(test)]
    pub(crate) fn simulate_failed_reconnect_for_test(&mut self) {
        self.reconnect_after = None;
        self.session = None;
        self.schedule_reconnect();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use picoo_protocol::control::ReceiverStats as ReceiverStatsMsg;
    use picoo_protocol::control::{PairingChallenge, ServerHello};
    use picoo_protocol::ALPN;
    use picoo_rate_control::BitrateAction;
    use picoo_session::SenderStatus;
    use picoo_testkit::MemoryTransport;
    use picoo_transport::{CloseReason, Endpoint};
    use prost::Message;

    use super::*;

    #[test]
    fn memory_transport_flush_pending() {
        let mut session = SenderSession::new(MemoryTransport::new());
        session
            .connect(Endpoint {
                host: "127.0.0.1".into(),
                port: 1,
            })
            .expect("connect");
        session
            .ingest_access_unit(b"au-bytes", true, 1, 1)
            .expect("ingest");
        let sent = session.flush_pending().expect("flush");
        assert_eq!(sent, 1);
        assert_eq!(session.stats().sent_datagrams, 1);
    }

    #[test]
    fn disconnected_media_is_rejected_and_pending_packets_are_cleared() {
        let mut session = SenderSession::new(MemoryTransport::new());
        assert!(matches!(
            session.ingest_access_unit(b"offline", true, 1, 1),
            Err(SenderError::NotConnected)
        ));
        assert_eq!(session.pending_packets(), 0);

        session
            .connect(Endpoint {
                host: "127.0.0.1".into(),
                port: 1,
            })
            .expect("connect");
        session
            .ingest_access_unit(b"queued", true, 2, 1)
            .expect("ingest while connected");
        assert_eq!(session.pending_packets(), 1);
        session.disconnect();
        assert_eq!(session.pending_packets(), 0);
    }

    #[test]
    fn reconnects_after_disconnect_with_backoff() {
        let mut session = SenderSession::new(MemoryTransport::new());
        let endpoint = Endpoint {
            host: "127.0.0.1".into(),
            port: 4433,
        };
        let _first = session.connect(endpoint.clone()).expect("connect");
        assert!(session.is_connected());

        session.disconnect_for_test(CloseReason::PeerClose);
        session.pump().expect("pump after disconnect");
        assert_eq!(session.status(), SenderStatus::Reconnecting);
        assert_eq!(session.last_scheduled_reconnect_delay_ms(), Some(500));

        for _ in 0..20 {
            session.pump().expect("reconnect pump");
            if session.is_connected() {
                break;
            }
            std::thread::sleep(Duration::from_millis(600));
        }
        assert!(session.is_connected());
        assert_ne!(session.status(), SenderStatus::Disconnected);
    }

    #[test]
    fn reconnect_backoff_escalates_across_failed_attempts() {
        // REQ-PICOO-TRANSPORT-004 / PUC-006: 500 → 1000 → 2000 → 5000 → 5000.
        let mut session = SenderSession::new(MemoryTransport::new());
        session
            .connect(Endpoint {
                host: "127.0.0.1".into(),
                port: 4433,
            })
            .expect("connect");

        session.disconnect_for_test(CloseReason::Timeout);
        session.pump().expect("pump");
        assert_eq!(session.status(), SenderStatus::Reconnecting);
        assert_eq!(session.last_scheduled_reconnect_delay_ms(), Some(500));
        assert_eq!(session.reconnect_attempt(), 1);

        session.simulate_failed_reconnect_for_test();
        assert_eq!(session.last_scheduled_reconnect_delay_ms(), Some(1_000));
        assert_eq!(session.reconnect_attempt(), 2);
        session.simulate_failed_reconnect_for_test();
        assert_eq!(session.last_scheduled_reconnect_delay_ms(), Some(2_000));
        session.simulate_failed_reconnect_for_test();
        assert_eq!(session.last_scheduled_reconnect_delay_ms(), Some(5_000));
        session.simulate_failed_reconnect_for_test();
        assert_eq!(session.last_scheduled_reconnect_delay_ms(), Some(5_000));
    }

    #[test]
    fn user_disconnect_stays_disconnected_without_reconnect() {
        // PUC-005: intentional stop must not bounce into Reconnecting.
        let mut session = SenderSession::new(MemoryTransport::new());
        session
            .connect(Endpoint {
                host: "127.0.0.1".into(),
                port: 4433,
            })
            .expect("connect");
        assert!(session.is_connected());

        session.disconnect();
        assert_eq!(session.status(), SenderStatus::Disconnected);
        assert!(!session.is_connected());

        for _ in 0..10 {
            session.pump().expect("pump");
            std::thread::sleep(Duration::from_millis(100));
        }
        assert_eq!(session.status(), SenderStatus::Disconnected);
        assert!(!session.is_connected());

        // Explicit connect must work again after user stop.
        session
            .connect(Endpoint {
                host: "127.0.0.1".into(),
                port: 4433,
            })
            .expect("reconnect after user stop");
        assert!(session.is_connected());
    }

    #[test]
    fn receiver_stats_adjusts_bitrate() {
        let mut session = SenderSession::new(MemoryTransport::new());
        let endpoint = Endpoint {
            host: "127.0.0.1".into(),
            port: 4433,
        };
        session.connect(endpoint).expect("connect");

        let stats = ReceiverStatsMsg {
            packet_loss: 0.05,
            ..Default::default()
        };
        let mut buf = Vec::new();
        stats.encode(&mut buf).expect("encode");
        session
            .inject_control_for_test(bytes::Bytes::from(buf))
            .expect("inject stats");
        session.pump().expect("pump");
        assert_eq!(session.last_bitrate_action(), BitrateAction::Decrease);
        assert!(session.current_bitrate_bps() < DEFAULT_INITIAL_BITRATE_BPS);
    }

    #[test]
    fn sustained_floor_congestion_requests_resolution_downshift() {
        let mut session = SenderSession::new(MemoryTransport::new());
        session
            .connect(Endpoint {
                host: "127.0.0.1".into(),
                port: 4433,
            })
            .expect("connect");
        // Drive bitrate to the floor first.
        for _ in 0..20 {
            let stats = ReceiverStatsMsg {
                packet_loss: 0.05,
                frame_age_ms: 250.0,
                ..Default::default()
            };
            let mut buf = Vec::new();
            stats.encode(&mut buf).expect("encode");
            session
                .inject_control_for_test(bytes::Bytes::from(buf))
                .expect("inject");
        }
        // Keep injecting while at floor until downshift fires.
        let mut saw = false;
        for _ in 0..10 {
            let stats = ReceiverStatsMsg {
                packet_loss: 0.05,
                frame_age_ms: 250.0,
                ..Default::default()
            };
            let mut buf = Vec::new();
            stats.encode(&mut buf).expect("encode");
            session
                .inject_control_for_test(bytes::Bytes::from(buf))
                .expect("inject");
            if session.take_resolution_downshift() {
                saw = true;
                break;
            }
        }
        assert!(
            saw,
            "expected resolution downshift after sustained floor congestion"
        );
        assert!(!session.take_resolution_downshift());
    }

    #[test]
    fn high_packet_loss_marks_network_unstable() {
        // REQ-PICOO-SESSION-001
        let mut session = SenderSession::new(MemoryTransport::new());
        session
            .connect(Endpoint {
                host: "127.0.0.1".into(),
                port: 4433,
            })
            .expect("connect");
        session.force_status_for_test(SenderStatus::Streaming);

        let high_loss = ReceiverStatsMsg {
            packet_loss: 0.05,
            ..Default::default()
        };
        let mut buf = Vec::new();
        high_loss.encode(&mut buf).expect("encode");
        session
            .inject_control_for_test(bytes::Bytes::from(buf))
            .expect("inject");
        assert_eq!(session.status(), SenderStatus::NetworkUnstable);

        let recovered = ReceiverStatsMsg {
            packet_loss: 0.005,
            ..Default::default()
        };
        let mut buf = Vec::new();
        recovered.encode(&mut buf).expect("encode");
        session
            .inject_control_for_test(bytes::Bytes::from(buf))
            .expect("inject");
        assert_eq!(session.status(), SenderStatus::Streaming);
    }

    #[test]
    fn mark_permission_required_is_observable() {
        let mut session = SenderSession::new(MemoryTransport::new());
        session.mark_permission_required();
        assert_eq!(session.status(), SenderStatus::PermissionRequired);
        session.clear_permission_required();
        assert_eq!(session.status(), SenderStatus::Disconnected);
    }

    #[test]
    fn resends_client_hello_after_reconnect() {
        let mut session = SenderSession::new(MemoryTransport::new());
        let endpoint = Endpoint {
            host: "127.0.0.1".into(),
            port: 4433,
        };
        session.connect(endpoint.clone()).expect("connect");
        session
            .send_client_hello("phone-1", "Pixel", &[1, 2, 3])
            .expect("hello");

        session.disconnect_for_test(CloseReason::Timeout);
        session.pump().expect("disconnect pump");

        for _ in 0..20 {
            session.pump().expect("reconnect pump");
            if session.is_connected() {
                break;
            }
            std::thread::sleep(Duration::from_millis(600));
        }
        assert!(session.is_connected());
    }

    #[test]
    fn resends_stream_config_and_requests_keyframe_after_reconnect() {
        let mut session = SenderSession::new(MemoryTransport::new());
        let endpoint = Endpoint {
            host: "127.0.0.1".into(),
            port: 4433,
        };
        session.connect(endpoint.clone()).expect("connect");
        session
            .send_client_hello("phone-1", "Pixel", &[1, 2, 3])
            .expect("hello");
        session.set_stream_config(StreamConfigParams {
            width: 1920,
            height: 1080,
            fps: 30,
            bitrate_bps: 6_000_000,
            stream_epoch: 2,
            mirrored: true,
            sps: vec![0x67, 0x42],
            pps: vec![0x68, 0xce],
            ..Default::default()
        });

        let hello = ServerHello {
            receiver_id: "recv-1".into(),
            display_name: "Desktop".into(),
            protocol_version: ALPN.into(),
            public_key: vec![9, 9],
            pairing_required: false,
        };
        let mut buf = Vec::new();
        hello.encode(&mut buf).expect("encode");
        session
            .inject_control_for_test(bytes::Bytes::from(buf))
            .expect("inject hello");
        assert_eq!(session.status(), SenderStatus::Streaming);
        assert_eq!(session.connected_receiver_id(), Some("recv-1"));
        assert_eq!(session.connected_receiver_display_name(), Some("Desktop"));
        assert!(session.stream_config_sent());
        assert!(session.take_keyframe_request());

        session.disconnect_for_test(CloseReason::PeerClose);
        session.pump().expect("disconnect pump");
        assert!(!session.stream_config_sent());

        for _ in 0..20 {
            session.pump().expect("reconnect pump");
            if session.is_connected() {
                break;
            }
            std::thread::sleep(Duration::from_millis(600));
        }
        assert!(session.is_connected());

        let hello2 = ServerHello {
            receiver_id: "recv-1".into(),
            display_name: "Desktop".into(),
            protocol_version: ALPN.into(),
            public_key: vec![9, 9],
            pairing_required: false,
        };
        let mut buf2 = Vec::new();
        hello2.encode(&mut buf2).expect("encode");
        session
            .inject_control_for_test(bytes::Bytes::from(buf2))
            .expect("inject hello2");
        session.pump().expect("pump streaming");

        assert_eq!(session.status(), SenderStatus::Streaming);
        assert!(session.stream_config_sent());
        let cfg = session.pending_stream_config().expect("config");
        assert_eq!(cfg.width, 1920);
        assert_eq!(cfg.height, 1080);
        assert!(cfg.mirrored);
        assert_eq!(cfg.sps, vec![0x67, 0x42]);
        assert_eq!(cfg.pps, vec![0x68, 0xce]);
        assert!(session.take_keyframe_request());
    }

    #[test]
    fn encoder_command_request_keyframe_sets_flag() {
        use picoo_protocol::control::encoder_command;
        use picoo_protocol::control::EncoderCommand;

        let mut session = SenderSession::new(MemoryTransport::new());
        session
            .connect(Endpoint {
                host: "127.0.0.1".into(),
                port: 1,
            })
            .expect("connect");
        let cmd = EncoderCommand {
            command: encoder_command::Command::RequestKeyframe as i32,
        };
        let mut buf = Vec::new();
        cmd.encode(&mut buf).expect("encode");
        session
            .inject_control_for_test(bytes::Bytes::from(buf))
            .expect("inject");
        assert!(session.take_keyframe_request());
        assert!(!session.take_keyframe_request());
    }

    #[test]
    fn pairing_confirm_persists_trusted_receiver() {
        use picoo_pairing::TrustedDeviceStore;

        let dir = tempfile::tempdir().expect("tempdir");
        let store_path = dir.path().join("trusted.json");

        let mut session = SenderSession::new(MemoryTransport::new())
            .with_trusted_store(&store_path)
            .expect("attach store");
        let endpoint = Endpoint {
            host: "127.0.0.1".into(),
            port: 4433,
        };
        session.connect(endpoint).expect("connect");
        session
            .send_client_hello("android-sender", "Pixel", &[1, 2, 3])
            .expect("client hello");

        let hello = ServerHello {
            receiver_id: "windows-receiver".into(),
            display_name: "Picoo Camera".into(),
            protocol_version: ALPN.into(),
            public_key: vec![4, 5, 6],
            pairing_required: true,
        };
        let mut buf = Vec::new();
        hello.encode(&mut buf).expect("encode hello");
        session
            .inject_control_for_test(bytes::Bytes::from(buf))
            .expect("inject hello");

        let challenge = PairingChallenge {
            short_code: "123456".into(),
            challenge_nonce: vec![0xABu8; 32],
        };
        let mut buf = Vec::new();
        challenge.encode(&mut buf).expect("encode challenge");
        session
            .inject_control_for_test(bytes::Bytes::from(buf))
            .expect("inject challenge");

        session
            .send_pairing_confirm("windows-receiver")
            .expect("confirm");

        assert_eq!(session.status(), SenderStatus::Streaming);
        assert!(
            session.take_keyframe_request(),
            "pairing confirm must request IDR before first encode"
        );

        let loaded = TrustedDeviceStore::load_from_path(&store_path).expect("load");
        assert!(loaded.is_paired("windows-receiver"));
    }
}
