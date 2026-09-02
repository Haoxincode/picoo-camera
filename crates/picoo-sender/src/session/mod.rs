//! Sender session: packetization + transport flush + reconnect + bitrate control.
//!
//! REQ-PICOO-SESSION-001, REQ-PICOO-TRANSPORT-004, REQ-PICOO-MEDIA-007

mod abr;
mod control;
mod lifecycle;
mod media;
mod pairing;
mod stream;

use std::path::PathBuf;
use std::time::Instant;

use picoo_metrics::ReceiverStats as MetricsReceiverStats;
use picoo_pairing::TrustedDeviceStore;
use picoo_protocol::control::{CameraCommand, Capabilities};
use picoo_rate_control::{BitrateAction, BitrateController};
use picoo_session::{ReconnectBackoff, SenderStatus};
use picoo_transport::{Endpoint, PicooTransport, SessionId};

use crate::stream_config::StreamConfigParams;
use crate::{SenderPipeline, SenderStats};

use pairing::{ClientHelloParams, SenderPairing};

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
    last_sender_stats_sent_at: Option<Instant>,
    pairing: Option<SenderPairing>,
    sender_id: Option<String>,
    hello_params: Option<ClientHelloParams>,
    trusted: TrustedDeviceStore,
    trusted_store_path: Option<PathBuf>,
    status: SenderStatus,
    /// Session state suspended by the platform camera permission gate.
    permission_resume_status: Option<SenderStatus>,
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
            last_sender_stats_sent_at: None,
            pairing: None,
            sender_id: None,
            hello_params: None,
            trusted: TrustedDeviceStore::new(),
            trusted_store_path: None,
            status: SenderStatus::Disconnected,
            permission_resume_status: None,
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
        self.pending_encoder_directive
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
}

#[cfg(test)]
mod tests;
