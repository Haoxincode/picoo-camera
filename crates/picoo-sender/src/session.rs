//! Sender session: packetization + transport flush + reconnect + bitrate control.
//!
//! REQ-PICOO-SESSION-001, REQ-PICOO-TRANSPORT-004, REQ-PICOO-MEDIA-007

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use picoo_metrics::ReceiverStats as MetricsReceiverStats;
use picoo_pairing::{
    pairing_confirm_signature, pairing_transcript_hash, trusted_device_from_pairing,
    TrustedDeviceStore,
};
use picoo_protocol::control::{
    camera_command, encoder_command, CameraCommand, Capabilities, ClientHello, EncoderCommand,
    PairingApproval, PairingChallenge, PairingCommit, PairingComplete, PairingConfirm,
    ReceiverStats as ReceiverStatsMsg, ServerHello, SessionError, StartStream, StopStream,
};
use picoo_protocol::VideoPacket;
use picoo_protocol::ALPN;
use picoo_rate_control::{BitrateAction, BitrateController, BitrateLadder};
use picoo_session::{ReconnectBackoff, SenderStatus};
use picoo_transport::{Endpoint, PicooTransport, SessionId, TransportEvent};
use prost::Message;

use crate::stream_config::StreamConfigParams;
use crate::{SenderError, SenderPipeline, SenderStats};

pub const INITIAL_STREAM_EPOCH: u32 = 1;
const PAIRING_APPROVAL_MAGIC: u32 = 0x5041_5056;
const PAIRING_COMMIT_MAGIC: u32 = 0x5043_4D54;
const PAIRING_COMPLETE_MAGIC: u32 = 0x5043_4D50;
const PAIRING_APPROVAL_PHASE: &[u8] = b"pairing-approval-v2";
const PAIRING_COMMIT_PHASE: &[u8] = b"pairing-commit-v2";
const PAIRING_COMPLETE_PHASE: &[u8] = b"pairing-complete-v2";
/// Mobile FFI exposes epochs as a positive signed 32-bit integer on Android.
pub const MAX_STREAM_EPOCH: u32 = i32::MAX as u32;

#[derive(Debug, Clone)]
struct SenderPairing {
    receiver_id: String,
    display_name: String,
    public_key: Vec<u8>,
    challenge_nonce: Vec<u8>,
    short_code: String,
    confirm_sent: bool,
    trust_committed: bool,
}

#[derive(Debug, Clone)]
struct ClientHelloParams {
    sender_id: String,
    device_name: String,
    public_key: Vec<u8>,
    protocol_version: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionStats {
    pub pipeline: SenderStats,
    pub sent_datagrams: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum EncoderDirectiveKind {
    AbrDownshift = 1,
    AbrUpshift = 2,
}

/// Rust-owned desired encoder transition. Reading it never acknowledges it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncoderDirective {
    pub id: u64,
    pub kind: EncoderDirectiveKind,
    pub target_height: u32,
    pub target_bitrate_bps: u32,
    pub stream_epoch: u32,
}

pub struct SenderSession<T: PicooTransport> {
    pipeline: SenderPipeline,
    transport: T,
    session: Option<SessionId>,
    sent_datagrams: u64,
    pairing: Option<SenderPairing>,
    sender_id: Option<String>,
    hello_params: Option<ClientHelloParams>,
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
    /// User/platform preference before applying the current receiver cap.
    requested_preferred_height: u32,
    last_bitrate_action: BitrateAction,
    last_receiver_stats: Option<MetricsReceiverStats>,
    pending_stream_config: Option<StreamConfigParams>,
    receiver_capabilities: Option<Capabilities>,
    stream_config_sent: bool,
    /// Receiver asked for IDR via EncoderCommand (REQ-PICOO-SESSION-003/004).
    keyframe_requested: bool,
    pending_encoder_directive: Option<EncoderDirective>,
    next_encoder_directive_id: u64,
    current_stream_epoch: u32,
    last_allocated_stream_epoch: u32,
    /// Zero until the platform reports its first actual encoder output.
    committed_encoder_height: u32,
    pending_local_stream_epoch: Option<u32>,
    reconfiguration_rollback: Option<(Option<StreamConfigParams>, bool)>,
    stream_config_staged_during_reconfiguration: bool,
    /// A committed epoch must not emit media until its matching StreamConfig
    /// has been queued on the reliable control stream.
    media_blocked_for_stream_config: bool,
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
            bitrate: BitrateController::for_height(1080),
            requested_preferred_height: 1080,
            last_bitrate_action: BitrateAction::Hold,
            last_receiver_stats: None,
            pending_stream_config: Some(StreamConfigParams::default()),
            receiver_capabilities: None,
            stream_config_sent: false,
            keyframe_requested: false,
            pending_encoder_directive: None,
            next_encoder_directive_id: 1,
            current_stream_epoch: INITIAL_STREAM_EPOCH,
            last_allocated_stream_epoch: INITIAL_STREAM_EPOCH,
            committed_encoder_height: 0,
            pending_local_stream_epoch: None,
            reconfiguration_rollback: None,
            stream_config_staged_during_reconfiguration: false,
            media_blocked_for_stream_config: false,
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

    pub fn set_stream_config(&mut self, mut config: StreamConfigParams) {
        // Allow re-send when SPS/PPS arrive late or resolution/mirror changes (PUC-005/006).
        config.stream_epoch = self.current_stream_epoch;
        self.pending_stream_config = Some(config);
        self.stream_config_sent = false;
        if self.reconfiguration_rollback.is_some() {
            self.stream_config_staged_during_reconfiguration = true;
        }
    }

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

    /// Host applied an encode height for the current Rust-owned generation.
    pub fn report_encoder_height(&mut self, height: u32, stream_epoch: u32) -> bool {
        if height == 0 {
            return false;
        }
        let normalized_height = picoo_rate_control::normalize_height(height);
        if height != normalized_height {
            return false;
        }
        if self.pending_local_stream_epoch == Some(stream_epoch) {
            self.commit_stream_epoch(stream_epoch, normalized_height);
        } else if self.pending_local_stream_epoch.is_some()
            || self.pending_encoder_directive.is_some()
            || stream_epoch != self.current_stream_epoch
        {
            return false;
        } else if self.committed_encoder_height == 0 {
            // Initial synchronization is allowed only for the StreamConfig
            // already associated with the committed epoch.
            let configured_height = self
                .pending_stream_config
                .as_ref()
                .map(|config| config.height);
            if configured_height != Some(height) {
                return false;
            }
            self.committed_encoder_height = normalized_height;
        } else if height != self.committed_encoder_height {
            // Any actual resolution change must use begin/apply/report so it
            // receives a fresh epoch and cannot mutate committed state.
            return false;
        }
        self.bitrate.sync_encode_height(height);
        true
    }

    pub fn current_stream_epoch(&self) -> u32 {
        self.current_stream_epoch
    }

    /// Allocate a fresh stream generation before a native encoder discontinuity.
    pub fn begin_stream_reconfiguration(&mut self) -> u32 {
        // The platform must explicitly ACK/NACK/cancel the existing transition.
        // Silently replacing it would let a late native callback commit the
        // wrong generation.
        if self.pending_local_stream_epoch.is_some() || self.pending_encoder_directive.is_some() {
            return 0;
        }
        let epoch = self.allocate_stream_epoch();
        if epoch == 0 {
            return 0;
        }
        self.begin_reconfiguration_transaction();
        self.pending_local_stream_epoch = Some(epoch);
        self.keyframe_requested = true;
        epoch
    }

    fn allocate_stream_epoch(&mut self) -> u32 {
        if self.last_allocated_stream_epoch >= MAX_STREAM_EPOCH {
            self.last_session_error = Some("STREAM_EPOCH_EXHAUSTED".into());
            return 0;
        }
        let Some(next) = self.last_allocated_stream_epoch.checked_add(1) else {
            self.last_session_error = Some("STREAM_EPOCH_EXHAUSTED".into());
            return 0;
        };
        self.last_allocated_stream_epoch = next;
        next
    }

    fn commit_stream_epoch(&mut self, epoch: u32, actual_height: u32) {
        // Keep only a config explicitly staged during this transaction and
        // matching the native encoder output. The old epoch's config must
        // never be relabelled and sent for the new epoch.
        let staged_config = self
            .stream_config_staged_during_reconfiguration
            .then(|| self.pending_stream_config.clone())
            .flatten()
            .filter(|config| config.height == actual_height)
            .map(|mut config| {
                config.stream_epoch = epoch;
                config
            });
        self.current_stream_epoch = epoch;
        self.pending_local_stream_epoch = None;
        self.committed_encoder_height = actual_height;
        self.pending_stream_config = staged_config;
        self.stream_config_sent = false;
        self.media_blocked_for_stream_config = true;
        self.keyframe_requested = true;
        self.reconfiguration_rollback = None;
        self.stream_config_staged_during_reconfiguration = false;
    }

    pub fn cancel_stream_reconfiguration(&mut self, stream_epoch: u32) -> bool {
        if self.pending_local_stream_epoch != Some(stream_epoch) {
            return false;
        }
        self.pending_local_stream_epoch = None;
        self.rollback_reconfiguration_transaction();
        true
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

    pub fn pending_encoder_directive(&self) -> Option<EncoderDirective> {
        self.pending_encoder_directive
    }

    /// Advance ABR state only after the platform confirms the encoder reconfiguration.
    pub fn acknowledge_encoder_directive(&mut self, id: u64, actual_height: u32) -> bool {
        let Some(directive) = self.pending_encoder_directive else {
            return false;
        };
        if directive.id != id
            || self.pending_local_stream_epoch.is_some()
            || directive.stream_epoch == self.current_stream_epoch
            || actual_height != directive.target_height
        {
            return false;
        }
        self.bitrate.sync_encode_height(actual_height);
        self.commit_stream_epoch(directive.stream_epoch, directive.target_height);
        self.pending_encoder_directive = None;
        true
    }

    /// Keep the active ladder unchanged and allow a later ReceiverStats tick to retry.
    pub fn reject_encoder_directive(&mut self, id: u64) -> bool {
        let Some(directive) = self.pending_encoder_directive else {
            return false;
        };
        if directive.id != id {
            return false;
        }
        let action = match directive.kind {
            EncoderDirectiveKind::AbrDownshift => BitrateAction::DownshiftResolution,
            EncoderDirectiveKind::AbrUpshift => BitrateAction::UpshiftResolution,
        };
        self.bitrate.reject_resolution_change(action);
        self.pending_encoder_directive = None;
        self.rollback_reconfiguration_transaction();
        true
    }

    fn queue_encoder_directive(&mut self, kind: EncoderDirectiveKind, target_height: u32) {
        if self.pending_encoder_directive.is_some() || self.pending_local_stream_epoch.is_some() {
            return;
        }
        let target_height = self.cap_to_receiver_height(target_height);
        if target_height == self.bitrate.active_height() {
            let action = match kind {
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
        self.next_encoder_directive_id = next_id;
        self.begin_reconfiguration_transaction();
        self.pending_encoder_directive = Some(EncoderDirective {
            id,
            kind,
            target_height,
            target_bitrate_bps: BitrateLadder::for_height(target_height).initial_bps,
            stream_epoch,
        });
    }

    fn abort_pending_reconfiguration(&mut self) {
        if let Some(directive) = self.pending_encoder_directive.take() {
            match directive.kind {
                EncoderDirectiveKind::AbrDownshift => self
                    .bitrate
                    .reject_resolution_change(BitrateAction::DownshiftResolution),
                EncoderDirectiveKind::AbrUpshift => self
                    .bitrate
                    .reject_resolution_change(BitrateAction::UpshiftResolution),
            }
        }
        self.pending_local_stream_epoch = None;
        self.rollback_reconfiguration_transaction();
    }

    fn begin_reconfiguration_transaction(&mut self) {
        debug_assert!(self.reconfiguration_rollback.is_none());
        self.reconfiguration_rollback =
            Some((self.pending_stream_config.clone(), self.stream_config_sent));
        self.stream_config_staged_during_reconfiguration = false;
    }

    fn rollback_reconfiguration_transaction(&mut self) {
        if let Some((config, sent)) = self.reconfiguration_rollback.take() {
            self.pending_stream_config = config;
            self.stream_config_sent = sent;
        }
        self.stream_config_staged_during_reconfiguration = false;
    }

    fn cap_to_receiver_height(&self, height: u32) -> u32 {
        let requested = picoo_rate_control::normalize_height(height);
        let maximum = self.receiver_max_height();
        if maximum == 0 {
            requested
        } else {
            requested.min(picoo_rate_control::normalize_height(maximum))
        }
    }

    fn clear_receiver_capabilities(&mut self) {
        self.receiver_capabilities = None;
        self.bitrate
            .set_preferred_height(self.requested_preferred_height);
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
        let previous = self.trusted.clone();
        let removed = self.trusted.remove(device_id);
        if removed {
            if let Err(error) = self.persist_trusted() {
                self.trusted = previous;
                return Err(error);
            }
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
        if self.stream_config_sent
            || self.pending_local_stream_epoch.is_some()
            || self.pending_encoder_directive.is_some()
        {
            return Ok(());
        }
        let Some(config) = self.pending_stream_config.clone() else {
            return Ok(());
        };
        if self.media_blocked_for_stream_config && config.height != self.committed_encoder_height {
            self.last_session_error = Some("STREAM_CONFIG_HEIGHT_MISMATCH".into());
            return Err(SenderError::StreamConfigHeightMismatch {
                expected: self.committed_encoder_height,
                got: config.height,
            });
        }
        self.send_stream_config(&config)?;
        self.stream_config_sent = true;
        self.media_blocked_for_stream_config = false;
        Ok(())
    }

    fn send_stream_config(&mut self, config: &StreamConfigParams) -> Result<(), SenderError> {
        let session = self.session.ok_or(SenderError::NotConnected)?;
        let mut config = config.clone();
        config.stream_epoch = self.current_stream_epoch;
        let msg = config.to_proto();
        let mut buf = Vec::new();
        msg.encode(&mut buf)
            .map_err(|e| SenderError::Protocol(e.to_string()))?;
        self.transport
            .send_control(session, bytes::Bytes::from(buf))
            .map_err(SenderError::Transport)?;
        self.pending_stream_config = Some(config);
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
        if let Some(params) = self.hello_params.clone() {
            if self.emit_client_hello(&params).is_ok() {
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
                TransportEvent::ControlMessage(session, msg) => self.handle_control(session, msg),
                TransportEvent::Disconnected(_, _) => {
                    self.abort_pending_reconfiguration();
                    self.session = None;
                    self.pairing = None;
                    self.stream_config_sent = false;
                    self.clear_receiver_capabilities();
                    self.pipeline.clear_pending_packets();
                    self.schedule_reconnect();
                }
                TransportEvent::VideoPacket(_, _) => {}
            }
        }
    }

    fn handle_control(&mut self, session: SessionId, msg: bytes::Bytes) {
        if let Ok(approval) = PairingApproval::decode(msg.as_ref()) {
            if approval.magic == PAIRING_APPROVAL_MAGIC
                && self.pairing_transcript_matches(
                    session,
                    &approval.challenge_nonce,
                    &approval.transcript_hash,
                    PAIRING_APPROVAL_PHASE,
                )
            {
                self.accept_pairing_approval();
                return;
            }
        }
        if let Ok(complete) = PairingComplete::decode(msg.as_ref()) {
            if complete.magic == PAIRING_COMPLETE_MAGIC
                && self.pairing_transcript_matches(
                    session,
                    &complete.challenge_nonce,
                    &complete.transcript_hash,
                    PAIRING_COMPLETE_PHASE,
                )
            {
                self.accept_pairing_complete();
                return;
            }
        }
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
            if self.pending_encoder_directive.is_none()
                && self.pending_local_stream_epoch.is_none()
                && matches!(
                    self.last_bitrate_action,
                    BitrateAction::DownshiftResolution | BitrateAction::UpshiftResolution
                )
            {
                if let Some(target_height) =
                    self.bitrate.target_height_for(self.last_bitrate_action)
                {
                    let kind = match self.last_bitrate_action {
                        BitrateAction::DownshiftResolution => EncoderDirectiveKind::AbrDownshift,
                        BitrateAction::UpshiftResolution => EncoderDirectiveKind::AbrUpshift,
                        _ => unreachable!(),
                    };
                    self.queue_encoder_directive(kind, target_height);
                }
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
                self.bitrate.set_preferred_height(
                    self.cap_to_receiver_height(self.requested_preferred_height),
                );
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
                    pairing.confirm_sent = false;
                    pairing.trust_committed = false;
                } else {
                    self.pairing = Some(SenderPairing {
                        receiver_id: String::new(),
                        display_name: String::new(),
                        public_key: Vec::new(),
                        challenge_nonce: challenge.challenge_nonce,
                        short_code: challenge.short_code,
                        confirm_sent: false,
                        trust_committed: false,
                    });
                }
                self.status = SenderStatus::Pairing;
                return;
            }
        }
        // Known SessionError codes before ServerHello — all use string field 1.
        if let Ok(err) = SessionError::decode(msg.as_ref()) {
            if matches!(
                err.code.as_str(),
                "UNPAIRED" | "PUBLIC_KEY_CHANGED" | "PAIRING_REJECTED"
            ) {
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
                        confirm_sent: false,
                        trust_committed: false,
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
                        confirm_sent: false,
                        trust_committed: false,
                    });
                }
                self.enter_streaming();
            }
        }
    }

    fn pairing_transcript_matches(
        &self,
        session: SessionId,
        nonce: &[u8],
        transcript_hash: &[u8],
        phase: &[u8],
    ) -> bool {
        let Some(pairing) = self.pairing.as_ref() else {
            return false;
        };
        let Some(sender_id) = self.sender_id.as_deref() else {
            return false;
        };
        self.session == Some(session)
            && nonce == pairing.challenge_nonce
            && transcript_hash
                == pairing_transcript_hash(
                    &pairing.challenge_nonce,
                    &pairing.receiver_id,
                    sender_id,
                    phase,
                )
    }

    fn accept_pairing_approval(&mut self) {
        if self.status != SenderStatus::Pairing {
            return;
        }
        let Some(pairing) = self.pairing.clone() else {
            self.last_session_error = Some("PAIRING_STATE_MISSING".into());
            return;
        };
        if pairing.receiver_id.is_empty() {
            self.last_session_error = Some("PAIRING_RECEIVER_ID_MISSING".into());
            return;
        }
        if !pairing.confirm_sent {
            self.last_session_error = Some("PAIRING_LOCAL_CONFIRM_MISSING".into());
            return;
        }

        if !pairing.trust_committed {
            let display_name = if pairing.display_name.is_empty() {
                pairing.receiver_id.as_str()
            } else {
                pairing.display_name.as_str()
            };
            let previous_trusted = self.trusted.clone();
            self.trusted.upsert(trusted_device_from_pairing(
                &pairing.receiver_id,
                display_name,
                &pairing.public_key,
                self.now_ms(),
            ));
            if self.persist_trusted().is_err() {
                self.trusted = previous_trusted;
                self.last_session_error = Some("PAIRING_STORE_FAILED".into());
                return;
            }
            if let Some(pairing) = self.pairing.as_mut() {
                pairing.trust_committed = true;
            }
        }

        let Some(active_session) = self.session else {
            self.last_session_error = Some("PAIRING_SESSION_MISSING".into());
            return;
        };
        let Some(sender_id) = self.sender_id.as_deref() else {
            self.last_session_error = Some("PAIRING_SENDER_ID_MISSING".into());
            return;
        };
        let commit = PairingCommit {
            magic: PAIRING_COMMIT_MAGIC,
            challenge_nonce: pairing.challenge_nonce.clone(),
            transcript_hash: pairing_transcript_hash(
                &pairing.challenge_nonce,
                &pairing.receiver_id,
                sender_id,
                PAIRING_COMMIT_PHASE,
            ),
        };
        let mut out = Vec::new();
        if commit.encode(&mut out).is_err()
            || self
                .transport
                .send_control(active_session, bytes::Bytes::from(out))
                .is_err()
        {
            self.last_session_error = Some("PAIRING_COMMIT_SEND_FAILED".into());
            return;
        }
        self.last_session_error = None;
    }

    fn accept_pairing_complete(&mut self) {
        if self.status != SenderStatus::Pairing {
            return;
        }
        let Some(pairing) = self.pairing.as_ref() else {
            return;
        };
        if !pairing.confirm_sent || !pairing.trust_committed {
            self.last_session_error = Some("PAIRING_COMMIT_MISSING".into());
            return;
        }
        if self
            .trusted
            .verify_paired_key(&pairing.receiver_id, &pairing.public_key)
            .is_err()
        {
            self.last_session_error = Some("PAIRING_STORE_MISMATCH".into());
            return;
        }
        self.last_session_error = None;
        self.enter_streaming();
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
        self.pipeline.clear_pending_packets();
        self.abort_pending_reconfiguration();
        self.stream_config_sent = false;
        self.clear_receiver_capabilities();
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
            self.send_pending_stream_config()?;
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
        if !matches!(
            self.status,
            SenderStatus::Streaming | SenderStatus::NetworkUnstable
        ) {
            self.pipeline.clear_pending_packets();
            return Err(SenderError::MediaNotReady);
        }
        if stream_epoch != self.current_stream_epoch {
            return Err(SenderError::StaleStreamEpoch {
                got: stream_epoch,
                current: self.current_stream_epoch,
            });
        }
        if self.media_blocked_for_stream_config {
            return Err(SenderError::StreamConfigPending {
                stream_epoch: self.current_stream_epoch,
            });
        }
        self.pipeline
            .ingest_access_unit(data, is_keyframe, pts_us, stream_epoch)
    }

    /// Send all pending VideoPackets over QUIC datagrams.
    pub fn flush_pending(&mut self) -> Result<usize, SenderError> {
        let session = self.session.ok_or(SenderError::NotConnected)?;
        if !matches!(
            self.status,
            SenderStatus::Streaming | SenderStatus::NetworkUnstable
        ) {
            self.pipeline.clear_pending_packets();
            return Err(SenderError::MediaNotReady);
        }
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

    /// Test-only malicious/legacy-peer hook: put media on the transport without changing
    /// the session's semantic status. Receiver security tests use this to prove that their
    /// independent pairing gate still rejects packets even if a peer ignores the sender gate.
    pub fn ingest_and_flush_unchecked_for_test(
        &mut self,
        data: &[u8],
        is_keyframe: bool,
        pts_us: u64,
        stream_epoch: u32,
    ) -> Result<usize, SenderError> {
        let previous_status = self.status;
        self.status = SenderStatus::Streaming;
        let result = self.ingest_and_flush(data, is_keyframe, pts_us, stream_epoch);
        self.status = previous_status;
        result
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
        let connection_pending = matches!(
            self.status,
            SenderStatus::Connecting | SenderStatus::Reconnecting
        );
        if self.session.is_none() && !connection_pending {
            return Err(SenderError::NotConnected);
        }

        self.last_session_error = None;
        self.sender_id = Some(sender_id.into());
        let params = ClientHelloParams {
            sender_id: sender_id.to_string(),
            device_name: device_name.to_string(),
            public_key: public_key.to_vec(),
            protocol_version: protocol_version.to_string(),
        };
        self.hello_params = Some(params.clone());

        // QUIC connect is asynchronous on mobile. Treat ClientHello as the desired
        // first control message and let `on_connected` emit it once a session exists.
        // This preserves the Android call order: connect() -> sendClientHello().
        if self.session.is_none() {
            return Ok(());
        }

        self.emit_client_hello(&params)?;
        self.drain_events();
        Ok(())
    }

    fn emit_client_hello(&mut self, params: &ClientHelloParams) -> Result<(), SenderError> {
        let session = self.session.ok_or(SenderError::NotConnected)?;
        let hello = ClientHello {
            sender_id: params.sender_id.clone(),
            device_name: params.device_name.clone(),
            protocol_version: params.protocol_version.clone(),
            public_key: params.public_key.clone(),
        };
        let mut buf = Vec::new();
        hello
            .encode(&mut buf)
            .map_err(|e| SenderError::Protocol(e.to_string()))?;
        self.transport
            .send_control(session, bytes::Bytes::from(buf))
            .map_err(SenderError::Transport)?;
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
        if pairing.receiver_id != receiver_id {
            return Err(SenderError::Protocol(
                "pairing receiver id does not match ServerHello".into(),
            ));
        }

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
        if let Some(pairing) = self.pairing.as_mut() {
            pairing.confirm_sent = true;
        }
        self.last_session_error = None;
        // The receiver may still be waiting for its local user. Trust and media start only
        // after its authenticated PairingComplete acknowledgement (REQ-PICOO-PAIRING-001).
        Ok(())
    }

    /// Inject a decoded control message (tests / ABR loopback harnesses).
    pub fn inject_control_for_test(&mut self, msg: bytes::Bytes) -> Result<(), SenderError> {
        // Non-transport unit harnesses use a synthetic session. Pairing tests that need to
        // verify session binding call `inject_control_for_session_for_test` explicitly.
        let session = self.session.unwrap_or(SessionId(0));
        self.handle_control(session, msg);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn inject_control_for_session_for_test(
        &mut self,
        session: SessionId,
        msg: bytes::Bytes,
    ) {
        self.handle_control(session, msg);
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
    use std::collections::VecDeque;
    use std::time::Duration;

    use bytes::Bytes;
    use picoo_protocol::control::ReceiverStats as ReceiverStatsMsg;
    use picoo_protocol::control::{Capabilities, PairingChallenge, Resolution, ServerHello};
    use picoo_protocol::VideoPacket;
    use picoo_protocol::ALPN;
    use picoo_rate_control::BitrateAction;
    use picoo_session::SenderStatus;
    use picoo_testkit::MemoryTransport;
    use picoo_transport::{
        CloseReason, Endpoint, PicooTransport, SessionId, TransportError, TransportEvent,
    };
    use prost::Message;

    use super::*;

    struct DeferredConnectTransport {
        session: SessionId,
        connected: bool,
        events: VecDeque<TransportEvent>,
        sent_control: Vec<Bytes>,
    }

    impl DeferredConnectTransport {
        fn new() -> Self {
            Self {
                session: SessionId(1),
                connected: false,
                events: VecDeque::new(),
                sent_control: Vec::new(),
            }
        }

        fn complete_connect(&mut self) {
            self.connected = true;
            self.events
                .push_back(TransportEvent::Connected(self.session));
        }
    }

    impl PicooTransport for DeferredConnectTransport {
        fn connect(&mut self, _endpoint: Endpoint) -> Result<SessionId, TransportError> {
            Ok(self.session)
        }

        fn send_control(
            &mut self,
            session: SessionId,
            message: Bytes,
        ) -> Result<(), TransportError> {
            if !self.connected || session != self.session {
                return Err(TransportError::NotConnected);
            }
            self.sent_control.push(message);
            Ok(())
        }

        fn send_video(
            &mut self,
            _session: SessionId,
            _packet: VideoPacket,
        ) -> Result<(), TransportError> {
            Ok(())
        }

        fn poll_event(&mut self) -> Option<TransportEvent> {
            self.events.pop_front()
        }

        fn close(&mut self, session: SessionId, reason: CloseReason) {
            self.connected = false;
            self.events
                .push_back(TransportEvent::Disconnected(session, reason));
        }
    }

    #[test]
    fn client_hello_queued_before_async_connect_is_sent_when_connected() {
        // REQ-PICOO-DISCOVERY-007: mirrors Android connect() -> sendClientHello().
        let mut sender = SenderSession::new(DeferredConnectTransport::new());
        sender
            .connect(Endpoint {
                host: "192.168.8.101".into(),
                port: 4433,
            })
            .expect("queue connect");

        sender
            .send_client_hello("android-sender", "Pixel", &[1, 2, 3])
            .expect("queue hello before QUIC handshake completes");
        assert!(sender.transport().sent_control.is_empty());

        sender.transport_mut().complete_connect();
        sender.pump().expect("process connected event");

        assert_eq!(sender.status(), SenderStatus::Negotiating);
        let encoded = sender
            .transport()
            .sent_control
            .first()
            .expect("ClientHello emitted after connect");
        let hello = ClientHello::decode(encoded.as_ref()).expect("decode ClientHello");
        assert_eq!(hello.sender_id, "android-sender");
        assert_eq!(hello.protocol_version, ALPN);
    }

    #[test]
    fn memory_transport_flush_pending() {
        let mut session = SenderSession::new(MemoryTransport::new());
        session
            .connect(Endpoint {
                host: "127.0.0.1".into(),
                port: 1,
            })
            .expect("connect");
        session.force_status_for_test(SenderStatus::Streaming);
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
        session.force_status_for_test(SenderStatus::Streaming);
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
        assert!(session.current_bitrate_bps() < BitrateLadder::for_height(1080).initial_bps);
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
            if session.pending_encoder_directive().is_some() {
                saw = true;
                break;
            }
        }
        assert!(
            saw,
            "expected resolution downshift after sustained floor congestion"
        );
        let directive = session.pending_encoder_directive().expect("directive");
        assert_eq!(directive.kind, EncoderDirectiveKind::AbrDownshift);
        assert_eq!(directive.target_height, 720);
        assert_eq!(session.bitrate_active_height(), 1080);
        assert_eq!(session.pending_encoder_directive(), Some(directive));
        assert!(session.acknowledge_encoder_directive(directive.id, 720));
        assert_eq!(session.bitrate_active_height(), 720);
        assert!(session.pending_encoder_directive().is_none());
    }

    #[test]
    fn rejected_encoder_directive_keeps_active_height_and_can_retry() {
        let mut session = SenderSession::new(MemoryTransport::new());
        session
            .connect(Endpoint {
                host: "127.0.0.1".into(),
                port: 4433,
            })
            .expect("connect");
        for _ in 0..30 {
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
            if session.pending_encoder_directive().is_some() {
                break;
            }
        }
        let first = session
            .pending_encoder_directive()
            .expect("first directive");
        assert!(session.reject_encoder_directive(first.id));
        assert_eq!(session.bitrate_active_height(), 1080);

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
            if session.pending_encoder_directive().is_some() {
                break;
            }
        }
        let retry = session
            .pending_encoder_directive()
            .expect("retry directive");
        assert_ne!(retry.id, first.id);
        assert_eq!(retry.target_height, 720);
        assert_eq!(session.bitrate_active_height(), 1080);

        assert_eq!(session.begin_stream_reconfiguration(), 0);
        assert_eq!(session.pending_encoder_directive(), Some(retry));
        assert!(session.reject_encoder_directive(retry.id));
        let local_epoch = session.begin_stream_reconfiguration();
        assert!(local_epoch > retry.stream_epoch);
        assert_eq!(session.begin_stream_reconfiguration(), 0);
        assert_eq!(session.bitrate_active_height(), 1080);
    }

    #[test]
    fn stale_access_unit_epoch_is_rejected_after_reconfiguration_begins() {
        let mut session = SenderSession::new(MemoryTransport::new());
        session
            .connect(Endpoint {
                host: "127.0.0.1".into(),
                port: 4433,
            })
            .expect("connect");
        session.force_status_for_test(SenderStatus::Streaming);
        let committed_epoch = session.current_stream_epoch();
        let pending_epoch = session.begin_stream_reconfiguration();
        assert_ne!(pending_epoch, committed_epoch);
        session
            .ingest_access_unit(b"still-current", true, 1, committed_epoch)
            .expect("committed epoch remains valid while apply is pending");
        assert!(matches!(
            session.ingest_access_unit(b"not-committed", true, 2, pending_epoch),
            Err(SenderError::StaleStreamEpoch { got, current })
                if got == pending_epoch && current == committed_epoch
        ));
        assert!(session.report_encoder_height(720, pending_epoch));
        assert_eq!(session.current_stream_epoch(), pending_epoch);
        assert!(matches!(
            session.ingest_access_unit(b"now-stale", true, 3, committed_epoch),
            Err(SenderError::StaleStreamEpoch { got, current })
                if got == committed_epoch && current == pending_epoch
        ));
        assert!(matches!(
            session.ingest_access_unit(b"before-config", true, 4, pending_epoch),
            Err(SenderError::StreamConfigPending { stream_epoch })
                if stream_epoch == pending_epoch
        ));
        session.set_stream_config(StreamConfigParams {
            width: 1280,
            height: 720,
            ..Default::default()
        });
        session
            .send_pending_stream_config()
            .expect("queue matching config before media");
        session
            .ingest_access_unit(b"current", true, 5, pending_epoch)
            .expect("committed pending epoch accepted");
    }

    #[test]
    fn stream_config_epoch_changes_only_when_native_apply_commits() {
        let mut session = SenderSession::new(MemoryTransport::new());
        session
            .connect(Endpoint {
                host: "127.0.0.1".into(),
                port: 4433,
            })
            .expect("connect");
        session.stream_config_sent = true;
        let committed = session.current_stream_epoch();
        let pending = session.begin_stream_reconfiguration();
        assert_ne!(pending, committed);
        assert!(session.stream_config_sent());
        assert_eq!(
            session
                .pending_stream_config()
                .map(|config| config.stream_epoch),
            Some(committed)
        );

        assert!(session.report_encoder_height(720, pending));
        assert!(!session.stream_config_sent());
        assert!(session.pending_stream_config().is_none());
        assert!(session.media_blocked_for_stream_config);

        session.set_stream_config(StreamConfigParams {
            width: 1280,
            height: 720,
            ..Default::default()
        });
        session
            .send_pending_stream_config()
            .expect("new config is sent");
        assert_eq!(
            session
                .pending_stream_config()
                .map(|config| config.stream_epoch),
            Some(pending)
        );
        assert!(!session.media_blocked_for_stream_config);
    }

    #[test]
    fn current_epoch_report_is_idempotent_not_a_resolution_transition() {
        let mut session = SenderSession::new(MemoryTransport::new());
        let epoch = session.current_stream_epoch();
        session.set_stream_config(StreamConfigParams {
            width: 1920,
            height: 1080,
            ..Default::default()
        });
        assert!(session.report_encoder_height(1080, epoch));
        assert!(session.report_encoder_height(1080, epoch));
        assert!(!session.report_encoder_height(720, epoch));
        assert_eq!(session.bitrate_active_height(), 1080);
    }

    #[test]
    fn stream_epoch_exhausts_before_crossing_android_signed_range() {
        let mut session = SenderSession::new(MemoryTransport::new());
        session.last_allocated_stream_epoch = MAX_STREAM_EPOCH;
        assert_eq!(session.begin_stream_reconfiguration(), 0);
        assert_eq!(session.current_stream_epoch(), INITIAL_STREAM_EPOCH);
        assert_eq!(session.last_session_error(), Some("STREAM_EPOCH_EXHAUSTED"));
    }

    #[test]
    fn receiver_capability_caps_preferred_height_in_rust() {
        let mut session = SenderSession::new(MemoryTransport::new());
        let capabilities = Capabilities {
            codecs: vec!["h264".into()],
            resolutions: vec![
                Resolution {
                    width: 854,
                    height: 480,
                },
                Resolution {
                    width: 1280,
                    height: 720,
                },
            ],
            fps: vec![30],
            front_camera: true,
            back_camera: true,
        };
        let mut encoded = Vec::new();
        capabilities.encode(&mut encoded).expect("encode");
        session
            .inject_control_for_test(bytes::Bytes::from(encoded))
            .expect("inject capabilities");
        session.set_preferred_height(1080);
        assert_eq!(session.receiver_max_height(), 720);
        assert_eq!(session.bitrate.preferred_height(), 720);

        let expanded = Capabilities {
            codecs: vec!["h264".into()],
            resolutions: vec![Resolution {
                width: 1920,
                height: 1080,
            }],
            fps: vec![30],
            front_camera: true,
            back_camera: true,
        };
        let mut encoded = Vec::new();
        expanded.encode(&mut encoded).expect("encode");
        session
            .inject_control_for_test(bytes::Bytes::from(encoded))
            .expect("inject expanded capabilities");
        assert_eq!(session.bitrate.preferred_height(), 1080);
    }

    #[test]
    fn matching_config_staged_during_apply_is_kept_for_new_epoch() {
        let mut session = SenderSession::new(MemoryTransport::new());
        let pending = session.begin_stream_reconfiguration();
        session.set_stream_config(StreamConfigParams {
            width: 1280,
            height: 720,
            sps: vec![1, 2, 3],
            ..Default::default()
        });
        assert!(session.report_encoder_height(720, pending));
        let config = session.pending_stream_config().expect("staged config");
        assert_eq!(config.stream_epoch, pending);
        assert_eq!(config.sps, vec![1, 2, 3]);
        assert!(session.media_blocked_for_stream_config);
    }

    #[test]
    fn wrong_height_config_cannot_open_committed_epoch_media_gate() {
        let mut session = SenderSession::new(MemoryTransport::new());
        session
            .connect(Endpoint {
                host: "127.0.0.1".into(),
                port: 4433,
            })
            .expect("connect");
        session.force_status_for_test(SenderStatus::Streaming);
        let pending = session.begin_stream_reconfiguration();
        assert!(session.report_encoder_height(720, pending));
        session.set_stream_config(StreamConfigParams {
            width: 1920,
            height: 1080,
            ..Default::default()
        });
        assert!(matches!(
            session.send_pending_stream_config(),
            Err(SenderError::StreamConfigHeightMismatch {
                expected: 720,
                got: 1080
            })
        ));
        assert!(matches!(
            session.ingest_access_unit(b"blocked", true, 1, pending),
            Err(SenderError::StreamConfigPending { .. })
        ));
    }

    #[test]
    fn noncanonical_encoder_height_cannot_commit_ladder_epoch() {
        let mut session = SenderSession::new(MemoryTransport::new());
        let pending = session.begin_stream_reconfiguration();
        session.set_stream_config(StreamConfigParams {
            width: 1280,
            height: 800,
            ..Default::default()
        });
        assert!(!session.report_encoder_height(800, pending));
        assert_eq!(session.current_stream_epoch(), INITIAL_STREAM_EPOCH);
    }

    #[test]
    fn cancelled_reconfiguration_restores_committed_stream_config() {
        let mut session = SenderSession::new(MemoryTransport::new());
        session.stream_config_sent = true;
        let committed = session.pending_stream_config().cloned();
        let pending = session.begin_stream_reconfiguration();
        session.set_stream_config(StreamConfigParams {
            width: 854,
            height: 480,
            ..Default::default()
        });
        assert_eq!(session.pending_stream_config().map(|c| c.height), Some(480));
        assert!(session.cancel_stream_reconfiguration(pending));
        assert_eq!(session.pending_stream_config().cloned(), committed);
        assert!(session.stream_config_sent());
    }

    #[test]
    fn disconnect_aborts_pending_local_and_directive_generations() {
        let mut session = SenderSession::new(MemoryTransport::new());
        let local = session.begin_stream_reconfiguration();
        assert_eq!(session.pending_local_stream_epoch, Some(local));
        session.disconnect();
        assert_eq!(session.pending_local_stream_epoch, None);

        session.queue_encoder_directive(EncoderDirectiveKind::AbrDownshift, 720);
        assert!(session.pending_encoder_directive().is_some());
        session.disconnect();
        assert!(session.pending_encoder_directive().is_none());
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
    fn pairing_confirm_waits_for_receiver_completion() {
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

        let challenge_nonce = vec![0xABu8; 32];
        let challenge = PairingChallenge {
            short_code: "123456".into(),
            challenge_nonce: challenge_nonce.clone(),
        };
        let mut buf = Vec::new();
        challenge.encode(&mut buf).expect("encode challenge");
        session
            .inject_control_for_test(bytes::Bytes::from(buf))
            .expect("inject challenge");
        let _ = session.take_keyframe_request();

        let approval = PairingApproval {
            magic: PAIRING_APPROVAL_MAGIC,
            challenge_nonce: challenge_nonce.clone(),
            transcript_hash: pairing_transcript_hash(
                &challenge_nonce,
                "windows-receiver",
                "android-sender",
                PAIRING_APPROVAL_PHASE,
            ),
        };
        let mut approval_buf = Vec::new();
        approval.encode(&mut approval_buf).expect("encode approval");
        session
            .inject_control_for_test(bytes::Bytes::copy_from_slice(&approval_buf))
            .expect("inject premature approval");
        assert_eq!(session.status(), SenderStatus::Pairing);
        assert_eq!(
            session.last_session_error(),
            Some("PAIRING_LOCAL_CONFIRM_MISSING")
        );
        assert!(!session.trusted_devices().is_paired("windows-receiver"));

        session
            .send_pairing_confirm("windows-receiver")
            .expect("confirm");

        assert_eq!(session.status(), SenderStatus::Pairing);
        assert!(!session.trusted_devices().is_paired("windows-receiver"));
        assert!(!session.take_keyframe_request());
        assert!(matches!(
            session.ingest_access_unit(b"must-not-send", true, 1, INITIAL_STREAM_EPOCH),
            Err(SenderError::MediaNotReady)
        ));
        assert_eq!(session.pending_packets(), 0);

        let active_session = session.session.expect("active session");
        session.inject_control_for_session_for_test(
            SessionId(active_session.0 + 1),
            bytes::Bytes::copy_from_slice(&approval_buf),
        );
        assert!(!session.trusted_devices().is_paired("windows-receiver"));

        session
            .inject_control_for_test(bytes::Bytes::from(approval_buf))
            .expect("inject approval");
        assert_eq!(session.status(), SenderStatus::Pairing);
        assert!(session.trusted_devices().is_paired("windows-receiver"));
        assert!(!session.take_keyframe_request());

        let complete = PairingComplete {
            magic: PAIRING_COMPLETE_MAGIC,
            challenge_nonce: challenge_nonce.clone(),
            transcript_hash: pairing_transcript_hash(
                &challenge_nonce,
                "windows-receiver",
                "android-sender",
                PAIRING_COMPLETE_PHASE,
            ),
        };
        let mut complete_buf = Vec::new();
        complete
            .encode(&mut complete_buf)
            .expect("encode completion");
        session
            .inject_control_for_test(bytes::Bytes::from(complete_buf))
            .expect("inject completion");

        assert_eq!(session.status(), SenderStatus::Streaming);
        assert!(
            session.take_keyframe_request(),
            "receiver completion must request IDR before first encode"
        );

        let loaded = TrustedDeviceStore::load_from_path(&store_path).expect("load");
        assert!(loaded.is_paired("windows-receiver"));
    }

    #[test]
    fn failed_trusted_device_persist_rolls_back_memory_state() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut session = SenderSession::new(MemoryTransport::new());
        session.trusted.upsert(picoo_pairing::TrustedDevice {
            device_id: "receiver-rollback".into(),
            device_name: "Receiver".into(),
            public_key: vec![1, 2, 3],
            certificate_fingerprint: "rollback".into(),
            paired_at_ms: 1,
            last_connected_at_ms: None,
        });
        // Writing JSON to a directory is guaranteed to fail. The in-memory
        // trust decision must remain unchanged when persistence does not commit.
        session.trusted_store_path = Some(dir.path().to_path_buf());

        assert!(session.remove_trusted_device("receiver-rollback").is_err());
        assert!(session.trusted.is_paired("receiver-rollback"));
    }
}
