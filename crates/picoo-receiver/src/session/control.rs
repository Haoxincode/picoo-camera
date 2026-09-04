//! Control protocol: Start/StopStream, StreamConfig, Capabilities, camera, keyframe.
//!
//! REQ-PICOO-PROTOCOL-*, REQ-PICOO-MEDIA-002/003, REQ-PICOO-SESSION-003/004.

use super::recovery::RecoveryReason;
use super::ReceiverSession;
use crate::ReceiverError;
use picoo_packet::ReassemblyMap;
use picoo_protocol::control::{
    camera_command, control_envelope::Payload as ControlPayload, CameraCommand, Capabilities,
    EncoderCommand, Resolution, SenderStats as SenderStatsMsg, SessionError, StreamConfig,
};
use picoo_protocol::MAX_VIDEO_FRAGMENTS_PER_ACCESS_UNIT;
use picoo_session::StreamState;
use picoo_transport::{CloseReason, SessionId};

impl ReceiverSession {
    pub(crate) fn handle_control(
        &mut self,
        session: SessionId,
        msg: bytes::Bytes,
    ) -> Result<(), ReceiverError> {
        if self.transport.active_session() != Some(session) {
            return Err(ReceiverError::Protocol(
                "control event does not belong to the active transport session".into(),
            ));
        }
        let envelope = picoo_protocol::decode_control_envelope(&msg)
            .map_err(|error| ReceiverError::Protocol(error.to_string()))?;

        if self.control_generation.is_none() {
            if !matches!(envelope.payload, Some(ControlPayload::ClientHello(_))) {
                return Err(ReceiverError::Protocol(
                    "ClientHello must be the first PCP control payload".into(),
                ));
            }
            self.control_generation = Some(envelope.connection_generation);
        }
        if self.control_generation != Some(envelope.connection_generation) {
            return Err(ReceiverError::Protocol(
                "stale control connection_generation".into(),
            ));
        }
        if envelope.message_id <= self.last_received_control_message_id {
            return Err(ReceiverError::Protocol(
                "duplicate or out-of-order control message_id".into(),
            ));
        }
        self.last_received_control_message_id = envelope.message_id;

        let payload = envelope.payload.expect("validated envelope payload");
        if self.pending_pairing.is_some() {
            return match payload {
                ControlPayload::PairingCommit(commit) => {
                    self.handle_pairing_commit(session, commit)
                }
                ControlPayload::PairingConfirm(confirm) => {
                    self.handle_pairing_confirm(session, confirm)
                }
                ControlPayload::StartStream(_) => self.handle_start_stream(session),
                ControlPayload::StopStream(_) => self.handle_stop_stream(session),
                _ => Err(ReceiverError::Protocol(
                    "control payload is not allowed while pairing".into(),
                )),
            };
        }
        if self.active_sender.is_none() {
            return match payload {
                ControlPayload::ClientHello(hello) => self.handle_client_hello(session, hello),
                _ => Err(ReceiverError::Protocol(
                    "control payload is not valid before ClientHello".into(),
                )),
            };
        }
        if !self.video_allowed() {
            return Err(ReceiverError::Protocol(
                "control payload requires an authenticated sender".into(),
            ));
        }
        match payload {
            ControlPayload::StartStream(_) => self.handle_start_stream(session),
            ControlPayload::StopStream(_) => self.handle_stop_stream(session),
            ControlPayload::SenderStats(stats) => self.handle_sender_stats(stats),
            ControlPayload::StreamConfig(config) => self.handle_stream_config(session, config),
            _ => Err(ReceiverError::Protocol(
                "control payload is not allowed in the authenticated receiver phase".into(),
            )),
        }
    }

    fn handle_sender_stats(&mut self, stats: SenderStatsMsg) -> Result<(), ReceiverError> {
        if !stats.video_queue_age_ms.is_finite() || stats.video_queue_age_ms < 0.0 {
            return Ok(());
        }
        tracing::info!(
            sender_access_units = stats.access_units,
            submitted_datagrams = stats.submitted_datagrams,
            sender_queue_age_ms = stats.video_queue_age_ms,
            sender_queue_dropped_access_units = stats.video_dropped_access_units,
            sender_quic_lost_packets = stats.quic_lost_packets,
            sender_quic_sent_packets = stats.quic_sent_packets,
            sender_video_buffered_bytes = stats.video_buffered_bytes,
            "sender media window"
        );
        self.last_sender_stats = Some(stats);
        Ok(())
    }

    fn handle_start_stream(&mut self, session: SessionId) -> Result<(), ReceiverError> {
        if !self.video_allowed() {
            self.ingress.control_rejected_unpaired += 1;
            let err = SessionError {
                code: "UNPAIRED".into(),
                message: "StartStream rejected until pairing completes".into(),
            };
            let _ = self.send_control_payload(session, ControlPayload::SessionError(err));
            return Ok(());
        }
        self.begin_streaming(session)
    }

    pub(crate) fn handle_stop_stream(&mut self, session: SessionId) -> Result<(), ReceiverError> {
        // Unpaired / mid-pairing StopStream must not wipe the pairing challenge (PAIRING-003).
        if !self.video_allowed() {
            self.ingress.control_rejected_unpaired += 1;
            return Ok(());
        }
        self.decoder_worker.reset();
        self.frame_buffer_pool.clear();
        // Sender-initiated stop: tear down session video without auto-reconnect wait.
        self.active_sender = None;
        self.pending_pairing = None;
        self.current_stream_config = None;
        self.waiting_for_stream_config_epoch = None;
        self.pending_stream_config_idr = None;
        self.receiver_capabilities_sent = None;
        self.reassembly = ReassemblyMap::new(8, MAX_VIDEO_FRAGMENTS_PER_ACCESS_UNIT);
        self.stats_reporter = super::StatsReporter::new();
        self.jitter.clear();
        self.interarrival_jitter.reset();
        self.reset_network_health();
        self.last_stats = None;
        self.last_sender_stats = None;
        self.last_decoded_fps = 0;
        self.decoder_recovery.reset_session();
        self.placeholder_after = None;
        let _ = self.publish_waiting_placeholder();
        self.transport.close(session, CloseReason::LocalClose);
        self.reset_runtime_to_idle();
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
        self.send_control_payload(session, ControlPayload::CameraCommand(command))
    }

    fn handle_stream_config(
        &mut self,
        session: SessionId,
        config: StreamConfig,
    ) -> Result<(), ReceiverError> {
        let previous_epoch = self.current_stream_config.as_ref().map(|c| c.stream_epoch);
        if previous_epoch.is_some_and(|epoch| config.stream_epoch < epoch) {
            return Ok(());
        }
        let config_epoch = config.stream_epoch;
        let epoch_bumped = previous_epoch.is_some_and(|epoch| config.stream_epoch > epoch);
        self.current_stream_config = Some(std::sync::Arc::new(config));
        match self.waiting_for_stream_config_epoch {
            Some(waiting) if waiting == config_epoch => {
                self.waiting_for_stream_config_epoch = None;
            }
            Some(waiting) if waiting < config_epoch => {
                self.waiting_for_stream_config_epoch = None;
                self.pending_stream_config_idr = None;
            }
            Some(_) | None => {}
        }

        // Capability / StreamConfig exchange sits in Negotiating before live frames dominate UI.
        if self.video_allowed() && !self.runtime_state.stream().is_streaming() {
            self.runtime_state.set_stream(StreamState::Negotiating);
        }
        if self.receiver_capabilities_sent.is_none() {
            self.send_capabilities(session)?;
            self.receiver_capabilities_sent = Some(());
        }
        // After capabilities, paired receivers are ready to stream.
        if self.video_allowed() && self.runtime_state.stream() == StreamState::Negotiating {
            self.begin_streaming(session)?;
        }

        // PUC-005 / REQ-PICOO-MEDIA-003 / SESSION-004: request IDR on first
        // StreamConfig and on every stream_epoch bump so decoders recover quickly.
        let needs_keyframe = self.video_allowed() && (previous_epoch.is_none() || epoch_bumped);
        if needs_keyframe {
            let reason = if epoch_bumped {
                RecoveryReason::EpochChanged
            } else {
                RecoveryReason::InitialConfig
            };
            self.enter_decoder_recovery(reason, epoch_bumped)?;
        }
        self.release_pending_stream_config_idr(config_epoch)?;
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
        self.send_control_payload(session, ControlPayload::Capabilities(capabilities))
    }

    /// Ask Sender for an IDR after keyframe reassembly loss (REQ-PICOO-SESSION-003).
    pub(crate) fn send_request_keyframe_now(
        &mut self,
        session: SessionId,
    ) -> Result<(), ReceiverError> {
        let command = EncoderCommand {
            command: picoo_protocol::control::encoder_command::Command::RequestKeyframe as i32,
        };
        self.send_control_payload(session, ControlPayload::EncoderCommand(command))?;
        self.ingress.keyframe_requests = self.ingress.keyframe_requests.saturating_add(1);
        Ok(())
    }

    /// UI-triggered IDR request (REQ-PICOO-UI-003 live page).
    pub fn request_keyframe(&mut self) -> Result<(), ReceiverError> {
        if !self.video_allowed() {
            return Err(ReceiverError::Protocol(
                "RequestKeyframe requires paired streaming session".into(),
            ));
        }
        self.force_decoder_recovery_request(RecoveryReason::ManualRepair)
    }
}
