//! Control protocol: Start/StopStream, StreamConfig, Capabilities, camera, keyframe.
//!
//! REQ-PICOO-PROTOCOL-*, REQ-PICOO-MEDIA-002/003, REQ-PICOO-SESSION-003/004.

use super::recovery::RecoveryReason;
use super::ReceiverSession;
use crate::ReceiverError;
use picoo_protocol::control::{
    camera_command, control_envelope::Payload as ControlPayload, CameraCommand, Capabilities,
    ColorRange, DecoderOffer, EncoderCommand, FrameRate, Resolution, SenderStats as SenderStatsMsg,
    SessionError, StreamConfig, VideoCodec, VideoFormat,
};
use picoo_protocol::{receiver_payload_allowed, ReceiverControlPhase};
use picoo_session::StreamState;
use picoo_transport::SessionId;

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
        let phase = if self.pending_pairing.is_some() {
            ReceiverControlPhase::Pairing
        } else if self.active_sender.is_none() {
            ReceiverControlPhase::AwaitingClientHello
        } else if self.lifecycle.runtime.stream().is_streaming() {
            ReceiverControlPhase::Streaming
        } else {
            ReceiverControlPhase::AuthenticatedIdle
        };
        if !receiver_payload_allowed(phase, &payload) {
            return Err(ReceiverError::Protocol(
                "control payload is not allowed in the current receiver phase".into(),
            ));
        }
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
                _ => unreachable!("receiver phase gate covers pairing payloads"),
            };
        }
        if self.active_sender.is_none() {
            return match payload {
                ControlPayload::ClientHello(hello) => self.handle_client_hello(session, hello),
                _ => unreachable!("receiver phase gate requires ClientHello"),
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
            ControlPayload::ClockSyncPong(pong)
                if self.lifecycle.runtime.stream().is_streaming() =>
            {
                self.handle_clock_sync_pong(pong);
                Ok(())
            }
            _ => unreachable!("receiver phase gate covers authenticated payloads"),
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
        self.begin_streaming(session);
        Ok(())
    }

    pub(crate) fn handle_stop_stream(&mut self, session: SessionId) -> Result<(), ReceiverError> {
        // Unpaired / mid-pairing StopStream must not wipe the pairing challenge (PAIRING-003).
        if !self.video_allowed() {
            self.ingress.control_rejected_unpaired += 1;
            return Ok(());
        }
        self.apply_receiver_event(super::ReceiverEvent::StopStream {
            generation: session.0,
        })?;
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
        // REQ-PICOO-PROTOCOL-016: do not submit a different/unknown codec to
        // the current AVC-only adapter or mutate its committed configuration.
        if config.codec != VideoCodec::Avc as i32 {
            return Err(ReceiverError::Protocol("unsupported stream codec".into()));
        }
        let previous_epoch = self.current_stream_config.as_ref().map(|c| c.stream_epoch);
        if previous_epoch.is_some_and(|epoch| config.stream_epoch < epoch) {
            return Ok(());
        }
        let config_epoch = config.stream_epoch;
        let epoch_bumped = previous_epoch.is_some_and(|epoch| config.stream_epoch > epoch);
        self.config_revision = self.config_revision.checked_add(1).ok_or_else(|| {
            ReceiverError::Protocol("source configuration revision exhausted".into())
        })?;
        self.current_stream_config = Some(std::sync::Arc::new(config));
        #[cfg(target_os = "macos")]
        if let Some(output) = &self.shared_ring {
            output.invalidate();
        }
        if previous_epoch != Some(config_epoch) {
            self.reset_clock_sync(config_epoch);
        }
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
        if self.video_allowed() && !self.lifecycle.runtime.stream().is_streaming() {
            self.lifecycle.runtime.set_stream(StreamState::Negotiating);
        }
        if self.receiver_capabilities_sent.is_none() {
            self.send_capabilities(session)?;
            self.receiver_capabilities_sent = Some(());
        }
        // After capabilities, paired receivers are ready to stream.
        if self.video_allowed() && self.lifecycle.runtime.stream() == StreamState::Negotiating {
            self.begin_streaming(session);
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
        // This adapter currently implements AVC/30. Do not advertise HEVC/60
        // until the native decoder adapters have been integrated and validated.
        let offers = [(1280, 720), (1920, 1080)]
            .into_iter()
            .filter(|(_, height)| *height <= self.advertised_max_height)
            .map(|(width, height)| DecoderOffer {
                format: Some(VideoFormat::sdr_709(
                    VideoCodec::Avc,
                    Resolution { width, height },
                    FrameRate {
                        numerator: 30,
                        denominator: 1,
                    },
                    ColorRange::Limited,
                )),
                max_access_unit_bytes: picoo_protocol::MAX_MEDIA_ACCESS_UNIT_BYTES,
                max_level_idc: 42,
            })
            .collect();
        let capabilities = Capabilities { offers };
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn unsupported_codec_does_not_replace_committed_configuration_or_clock() {
        let mut receiver = ReceiverSession::new();
        let committed = Arc::new(StreamConfig {
            codec: VideoCodec::Avc as i32,
            stream_epoch: 5,
            width: 1280,
            height: 720,
            ..Default::default()
        });
        receiver.current_stream_config = Some(committed.clone());
        receiver.waiting_for_stream_config_epoch = Some(6);
        for codec in [0, -1, 99, VideoCodec::Hevc as i32] {
            let result = receiver.handle_stream_config(
                SessionId(1),
                StreamConfig {
                    codec,
                    stream_epoch: 6,
                    ..Default::default()
                },
            );
            assert!(
                matches!(result, Err(ReceiverError::Protocol(message)) if message == "unsupported stream codec")
            );
            assert!(Arc::ptr_eq(
                receiver.current_stream_config.as_ref().unwrap(),
                &committed
            ));
            assert_eq!(receiver.waiting_for_stream_config_epoch, Some(6));
        }
    }
}
