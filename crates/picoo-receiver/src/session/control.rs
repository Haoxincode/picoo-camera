//! Control protocol: Start/StopStream, StreamConfig, Capabilities, camera, keyframe.
//!
//! REQ-PICOO-PROTOCOL-*, REQ-PICOO-MEDIA-002/003, REQ-PICOO-SESSION-003/004.

use bytes::Bytes;
use picoo_packet::ReassemblyMap;
use picoo_pairing::verify_pairing_confirm;
use picoo_protocol::control::{
    camera_command, CameraCommand, Capabilities, EncoderCommand, PairingCommit, PairingConfirm,
    Resolution, SessionError, StartStream, StopStream, StreamConfig,
};
use picoo_protocol::MAX_VIDEO_FRAGMENTS_PER_ACCESS_UNIT;
use picoo_session::ReceiverStatus;
use picoo_transport::{CloseReason, SessionId};
use prost::Message;

use super::pairing::{PAIRING_COMMIT_MAGIC, PAIRING_COMMIT_PHASE};
use super::recovery::RecoveryReason;
use super::ReceiverSession;
use crate::ReceiverError;

impl ReceiverSession {
    pub(crate) fn handle_control(
        &mut self,
        session: SessionId,
        msg: Bytes,
    ) -> Result<(), ReceiverError> {
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

    pub(crate) fn handle_stop_stream(&mut self, session: SessionId) -> Result<(), ReceiverError> {
        // Unpaired / mid-pairing StopStream must not wipe the pairing challenge (PAIRING-003).
        if !self.video_allowed() {
            self.ingress.control_rejected_unpaired += 1;
            return Ok(());
        }
        // Finish protocol/session teardown before surfacing a decoder error.
        let decoder_reset = self.decoder.reset();
        // Sender-initiated stop: tear down session video without auto-reconnect wait.
        self.active_sender = None;
        self.pending_pairing = None;
        self.current_stream_config = None;
        self.waiting_for_stream_config_epoch = None;
        self.receiver_capabilities_sent = None;
        self.reassembly = ReassemblyMap::new(8, MAX_VIDEO_FRAGMENTS_PER_ACCESS_UNIT);
        self.stats_reporter = super::StatsReporter::new();
        self.jitter.clear();
        self.interarrival_jitter.reset();
        self.jitter_timeline = None;
        self.last_stats = None;
        self.last_decoded_fps = 0;
        self.decoder_recovery.reset_session();
        self.placeholder_after = None;
        let _ = self.publish_waiting_placeholder();
        self.transport.close(session, CloseReason::LocalClose);
        self.status = if self.bind_addr().is_some() {
            ReceiverStatus::Discovering
        } else {
            ReceiverStatus::Disconnected
        };
        decoder_reset?;
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
            let reason = if epoch_bumped {
                RecoveryReason::EpochChanged
            } else {
                RecoveryReason::InitialConfig
            };
            self.enter_decoder_recovery(reason, epoch_bumped)?;
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
    pub(crate) fn send_request_keyframe_now(
        &mut self,
        session: SessionId,
    ) -> Result<(), ReceiverError> {
        let command = EncoderCommand {
            command: picoo_protocol::control::encoder_command::Command::RequestKeyframe as i32,
        };
        self.send_control_message(session, &command)?;
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
