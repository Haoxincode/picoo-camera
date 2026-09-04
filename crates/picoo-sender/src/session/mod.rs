//! Sender session: packetization + transport flush + reconnect + bitrate control.
//!
//! REQ-PICOO-SESSION-001, REQ-PICOO-TRANSPORT-004, REQ-PICOO-MEDIA-007

mod abr;
mod clock;
mod control;
mod encoder_transaction;
mod lifecycle;
mod media;
mod pairing;
mod reducer;
mod stream;

use std::path::PathBuf;
use std::time::Instant;

use picoo_metrics::ReceiverStats as MetricsReceiverStats;
use picoo_pairing::{DeviceIdentity, TrustedDeviceStore};
use picoo_protocol::control::{
    control_envelope::Payload as ControlPayload, CameraCommand, Capabilities,
};
use picoo_rate_control::{BitrateAction, BitrateController};
use picoo_session::{ReconnectBackoff, SenderStatus, SessionRuntimeState};
use picoo_transport::{Endpoint, PicooTransport, SessionId};

use crate::stream_config::StreamConfigParams;
use crate::{SenderPipeline, SenderStats};

use encoder_transaction::EncoderApplyState;
use pairing::SenderPairing;
use reducer::{SenderEvent, SenderReducerState};

pub const INITIAL_STREAM_EPOCH: u32 = 1;
/// Mobile FFI exposes epochs as a positive signed 32-bit integer on Android.
pub const MAX_STREAM_EPOCH: u32 = i32::MAX as u32;

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
    Local = 3,
    Recovery = 4,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncoderFailureOutcome {
    Ignored,
    RolledBack,
    RecoveryRequested,
    Disconnected,
}

/// One immutable encoded-output fact reported by a native platform encoder.
#[derive(Debug, Clone, Copy)]
pub struct NativeEncoderAccessUnit<'a> {
    pub data: &'a [u8],
    pub is_keyframe: bool,
    pub pts_us: u64,
    pub encoded_at_us: u64,
    pub transaction_id: u64,
    pub encoder_generation: u64,
    pub stream_epoch: u32,
    pub height: u32,
}

pub struct SenderSession<T: PicooTransport> {
    pipeline: SenderPipeline,
    transport: T,
    sent_datagrams: u64,
    last_sender_stats_sent_at: Option<Instant>,
    identity: DeviceIdentity,
    pairing: Option<SenderPairing>,
    hello_requested: bool,
    sender_nonce: Option<[u8; 32]>,
    trusted: TrustedDeviceStore,
    trusted_store_path: Option<PathBuf>,
    lifecycle: SenderReducerState,
    last_endpoint: Option<Endpoint>,
    reconnect_backoff: ReconnectBackoff,
    reconnect_after: Option<Instant>,
    /// Last delay chosen by the reconnect Effect adapter (TRANSPORT-004 observability).
    last_scheduled_reconnect_delay_ms: Option<u64>,
    bitrate: BitrateController,
    /// User/platform preference before applying the current receiver cap.
    requested_preferred_height: u32,
    last_bitrate_action: BitrateAction,
    last_receiver_stats: Option<MetricsReceiverStats>,
    /// Raw video-fragment loss before successful FEC recovery. Kept separate
    /// from residual packet loss so FEC and ABR do not feed back on each other.
    pre_fec_packet_loss: f64,
    pending_stream_config: Option<StreamConfigParams>,
    receiver_capabilities: Option<Capabilities>,
    stream_config_sent: bool,
    /// Receiver asked for IDR via EncoderCommand (REQ-PICOO-SESSION-003/004).
    keyframe_requested: bool,
    encoder_apply_state: EncoderApplyState,
    next_encoder_directive_id: u64,
    current_stream_epoch: u32,
    last_allocated_stream_epoch: u32,
    /// Zero until the platform reports its first actual encoder output.
    committed_encoder_height: u32,
    /// Native generation bound by EncoderStarted; zero until the first encoder starts.
    committed_encoder_generation: u64,
    /// A committed epoch must not emit media until its matching StreamConfig
    /// has been queued on the reliable control stream.
    media_blocked_for_stream_config: bool,
    /// Latest CameraCommand from receiver (PUC-005 desktop remote control).
    pending_camera_command: Option<CameraCommand>,
    /// Last SessionError code from receiver (e.g. PUBLIC_KEY_CHANGED).
    last_session_error: Option<String>,
    next_control_message_id: u64,
    last_received_control_message_id: u64,
    /// Latest native media-clock sample used to answer clock sync pings in
    /// the same monotonic domain as source PTS.
    media_clock_anchor: Option<MediaClockAnchor>,
}

#[derive(Debug, Clone, Copy)]
struct MediaClockAnchor {
    stream_epoch: u32,
    encoded_at_us: u64,
    observed_at: Instant,
}

impl<T: PicooTransport> SenderSession<T> {
    pub fn new(transport: T) -> Self {
        let identity = DeviceIdentity::generate("Picoo Test Sender")
            .expect("OS CSPRNG must be available when constructing a Sender session");
        Self::new_with_identity(transport, identity)
    }

    /// Construct a product Sender with its durable platform-backed identity.
    pub fn new_with_identity(transport: T, identity: DeviceIdentity) -> Self {
        Self {
            pipeline: SenderPipeline::default(),
            transport,
            sent_datagrams: 0,
            last_sender_stats_sent_at: None,
            identity,
            pairing: None,
            hello_requested: false,
            sender_nonce: None,
            trusted: TrustedDeviceStore::new(),
            trusted_store_path: None,
            lifecycle: SenderReducerState::default(),
            last_endpoint: None,
            reconnect_backoff: ReconnectBackoff::default(),
            reconnect_after: None,
            last_scheduled_reconnect_delay_ms: None,
            bitrate: BitrateController::for_height(1080),
            requested_preferred_height: 1080,
            last_bitrate_action: BitrateAction::Hold,
            last_receiver_stats: None,
            pre_fec_packet_loss: 0.0,
            pending_stream_config: Some(StreamConfigParams::default()),
            receiver_capabilities: None,
            stream_config_sent: false,
            keyframe_requested: false,
            encoder_apply_state: EncoderApplyState::default(),
            next_encoder_directive_id: 1,
            current_stream_epoch: INITIAL_STREAM_EPOCH,
            last_allocated_stream_epoch: INITIAL_STREAM_EPOCH,
            committed_encoder_height: 0,
            committed_encoder_generation: 0,
            media_blocked_for_stream_config: false,
            pending_camera_command: None,
            last_session_error: None,
            next_control_message_id: 1,
            last_received_control_message_id: 0,
            media_clock_anchor: None,
        }
    }

    pub fn identity(&self) -> &DeviceIdentity {
        &self.identity
    }

    pub(super) fn send_control_payload(
        &mut self,
        session: SessionId,
        payload: ControlPayload,
    ) -> Result<(), crate::SenderError> {
        let message_id = self.next_control_message_id;
        self.next_control_message_id = self.next_control_message_id.saturating_add(1);
        let message = picoo_protocol::encode_control_envelope(payload, message_id, session.0);
        self.transport
            .send_control(session, message)
            .map_err(crate::SenderError::Transport)
    }

    pub fn status(&self) -> SenderStatus {
        self.lifecycle.runtime.sender_status()
    }

    pub fn runtime_state(&self) -> SessionRuntimeState {
        self.lifecycle.runtime
    }

    /// Access the underlying transport (loss injection / diagnostics in tests).
    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

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

    pub fn current_stream_epoch(&self) -> u32 {
        self.current_stream_epoch
    }

    pub fn stream_config_sent(&self) -> bool {
        self.stream_config_sent
    }

    pub fn pending_stream_config(&self) -> Option<&StreamConfigParams> {
        self.pending_stream_config.as_ref()
    }

    pub fn last_session_error(&self) -> Option<&str> {
        self.last_session_error.as_deref()
    }

    pub fn pending_encoder_directive(&self) -> Option<EncoderDirective> {
        self.encoder_apply_state
            .directive()
            .filter(|directive| directive.kind != EncoderDirectiveKind::Local)
    }

    pub fn stats(&self) -> SessionStats {
        SessionStats {
            pipeline: self.pipeline.stats(),
            sent_datagrams: self.sent_datagrams,
        }
    }

    pub fn is_connected(&self) -> bool {
        self.lifecycle.active_generation.is_some()
    }
}

#[cfg(test)]
mod tests;
