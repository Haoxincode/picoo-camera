use picoo_protocol::control::{camera_command, encoder_command, CameraCommand, EncoderCommand};
use picoo_session::{
    ConnectionState, HealthState, OutputState, SenderStatus, SessionRuntimeState, StreamState,
    TrustState,
};
use picoo_transport::{PicooTransport, SessionId};

use super::{EncoderDirectiveKind, NativeEncoderAccessUnit, SenderSession};
use crate::{FecProtection, SenderError};

impl<T: PicooTransport> SenderSession<T> {
    fn fec_protection_for(&self, is_keyframe: bool) -> FecProtection {
        let packet_loss = self.pre_fec_packet_loss;
        if is_keyframe || packet_loss >= 0.03 {
            FecProtection::Strong
        } else if packet_loss >= 0.01 {
            FecProtection::Light
        } else {
            FecProtection::None
        }
    }

    pub(super) fn enter_streaming(&mut self) {
        self.lifecycle.runtime.set_trust(TrustState::Authenticated);
        self.lifecycle.runtime.set_stream(StreamState::Streaming {
            generation: self.current_stream_epoch,
        });
        self.lifecycle.runtime.set_health(HealthState::Healthy);
        // Fresh streaming (including post-reconnect) needs an IDR (REQ-PICOO-SESSION-004).
        self.keyframe_requested = true;
        let _ = self.send_pending_stream_config();
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

    pub(super) fn handle_encoder_command(&mut self, command: &EncoderCommand) -> bool {
        if command.command == encoder_command::Command::RequestKeyframe as i32 {
            self.keyframe_requested = true;
            true
        } else {
            false
        }
    }

    pub(super) fn handle_camera_command(&mut self, cam: CameraCommand) -> bool {
        if cam.command != camera_command::Command::Unspecified as i32 {
            self.pending_camera_command = Some(cam);
            true
        } else {
            false
        }
    }

    pub fn ingest_encoder_access_unit(
        &mut self,
        access_unit: NativeEncoderAccessUnit<'_>,
    ) -> Result<usize, SenderError> {
        let NativeEncoderAccessUnit {
            data,
            is_keyframe,
            pts_us,
            transaction_id,
            encoder_generation,
            stream_epoch,
            height,
        } = access_unit;
        if data.is_empty() {
            return Err(SenderError::EmptyAccessUnit);
        }
        if self.active_session().is_none() {
            return Err(SenderError::NotConnected);
        }
        if !self.lifecycle.runtime.stream().is_streaming() {
            self.pipeline.clear_pending_packets();
            return Err(SenderError::MediaNotReady);
        }

        if self.encoder_apply_state.is_applying() {
            if !self.encoder_apply_state.matches_native_facts(
                transaction_id,
                encoder_generation,
                stream_epoch,
                height,
            ) {
                return Err(SenderError::EncoderRefreshPending);
            }
            if is_keyframe
                && (!self.encoder_apply_state.stream_config_staged()
                    || !self
                        .pending_stream_config
                        .as_ref()
                        .is_some_and(|config| config.height == height))
            {
                return Err(SenderError::StreamConfigPending { stream_epoch });
            }
            if is_keyframe {
                let mut committed_config = self
                    .pending_stream_config
                    .clone()
                    .ok_or(SenderError::StreamConfigPending { stream_epoch })?;
                committed_config.stream_epoch = stream_epoch;
                self.send_stream_config_for_epoch(&committed_config, stream_epoch)?;
            }
            if !is_keyframe {
                return Err(SenderError::EncoderRefreshPending);
            }
            // Queue the complete commit IDR before changing the committed
            // generation. Packetization can still reject an oversized AU or
            // an exhausted frame id; in either case the transaction must stay
            // pending so a later valid IDR can complete it.
            let fec = self.fec_protection_for(true);
            let packets =
                self.pipeline
                    .ingest_access_unit(data, true, pts_us, stream_epoch, fec)?;
            let Some(transaction) = self.encoder_apply_state.take_matching_keyframe(
                transaction_id,
                encoder_generation,
                stream_epoch,
                height,
                true,
            ) else {
                unreachable!("matching native facts were validated before packetization");
            };
            let mut committed_config = self
                .pending_stream_config
                .clone()
                .expect("matching IDR already validated its StreamConfig");
            committed_config.stream_epoch = stream_epoch;
            if transaction.directive.kind == EncoderDirectiveKind::Recovery {
                self.commit_encoder_recovery(
                    transaction,
                    height,
                    encoder_generation,
                    committed_config,
                );
            } else {
                self.bitrate.sync_encode_height(height);
                self.commit_stream_epoch(transaction, height, encoder_generation, committed_config);
            }
            self.keyframe_requested = false;
            return Ok(packets);
        } else if transaction_id != 0
            || encoder_generation == 0
            || encoder_generation != self.committed_encoder_generation
            || stream_epoch != self.current_stream_epoch
            || height != self.committed_encoder_height
        {
            return Err(SenderError::StaleEncoderFact);
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
        let fec = self.fec_protection_for(is_keyframe);
        let packets =
            self.pipeline
                .ingest_access_unit(data, is_keyframe, pts_us, stream_epoch, fec)?;
        if is_keyframe {
            self.keyframe_requested = false;
        }
        Ok(packets)
    }

    pub fn ingest_access_unit(
        &mut self,
        data: &[u8],
        is_keyframe: bool,
        pts_us: u64,
        stream_epoch: u32,
    ) -> Result<usize, SenderError> {
        if self.active_session().is_none() {
            return Err(SenderError::NotConnected);
        }
        if !self.lifecycle.runtime.stream().is_streaming() {
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
        let fec = self.fec_protection_for(is_keyframe);
        self.pipeline
            .ingest_access_unit(data, is_keyframe, pts_us, stream_epoch, fec)
    }

    /// Send all pending VideoPackets over QUIC datagrams.
    pub fn flush_pending(&mut self) -> Result<usize, SenderError> {
        let session = self.active_session().ok_or(SenderError::NotConnected)?;
        if !self.lifecycle.runtime.stream().is_streaming() {
            self.pipeline.clear_pending_packets();
            return Err(SenderError::MediaNotReady);
        }
        let batches = self.pipeline.take_pending_batches();
        let mut sent = 0;
        for batch in batches {
            let batch_len = batch.len();
            self.transport
                .send_video_batch(session, batch)
                .map_err(SenderError::Transport)?;
            sent += batch_len;
            self.sent_datagrams = self.sent_datagrams.saturating_add(batch_len as u64);
        }
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
        let previous_state = self.lifecycle.runtime;
        self.lifecycle
            .runtime
            .set_connection(ConnectionState::Connected {
                generation: self.active_session().map_or(0, |session| session.0),
            });
        self.lifecycle.runtime.set_trust(TrustState::Authenticated);
        self.lifecycle.runtime.set_stream(StreamState::Streaming {
            generation: stream_epoch,
        });
        let result = self.ingest_and_flush(data, is_keyframe, pts_us, stream_epoch);
        self.lifecycle.runtime = previous_state;
        result
    }

    pub fn pending_packets(&self) -> usize {
        self.pipeline.pending_datagram_count()
    }

    /// Inject a decoded control message (tests / ABR loopback harnesses).
    pub fn inject_control_for_test(&mut self, msg: bytes::Bytes) -> Result<(), SenderError> {
        // Non-transport unit harnesses use a synthetic session. Pairing tests that need to
        // verify session binding call `inject_control_for_session_for_test` explicitly.
        let session = self.active_session().unwrap_or(SessionId(0));
        self.handle_control(session, msg);
        Ok(())
    }

    pub fn inject_control_payload_for_test(
        &mut self,
        payload: picoo_protocol::control::control_envelope::Payload,
    ) -> Result<(), SenderError> {
        let session = self.active_session().ok_or(SenderError::NotConnected)?;
        let message = picoo_protocol::encode_control_envelope(
            payload,
            self.last_received_control_message_id.saturating_add(1),
            session.0,
        );
        self.handle_control(session, message);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn inject_control_payload_for_session_for_test(
        &mut self,
        session: SessionId,
        payload: picoo_protocol::control::control_envelope::Payload,
    ) {
        let active_generation = self.active_session().map_or(session.0, |active| active.0);
        let message = picoo_protocol::encode_control_envelope(
            payload,
            self.last_received_control_message_id.saturating_add(1),
            active_generation,
        );
        self.handle_control(session, message);
    }

    pub fn force_status_for_test(&mut self, status: SenderStatus) {
        self.lifecycle.runtime = SessionRuntimeState::default();
        let generation = self.active_session().map_or(1, |session| session.0);
        match status {
            SenderStatus::Disconnected => {}
            SenderStatus::Discovering => self
                .lifecycle
                .runtime
                .set_connection(ConnectionState::Listening),
            SenderStatus::Connecting => self
                .lifecycle
                .runtime
                .set_connection(ConnectionState::Connecting),
            SenderStatus::Reconnecting => self
                .lifecycle
                .runtime
                .set_connection(ConnectionState::Reconnecting { attempt: 1 }),
            SenderStatus::Pairing => {
                self.lifecycle
                    .runtime
                    .set_connection(ConnectionState::Connected { generation });
                self.lifecycle.runtime.set_trust(TrustState::Pairing);
                self.lifecycle.runtime.set_stream(StreamState::Negotiating);
            }
            SenderStatus::Negotiating => {
                self.lifecycle
                    .runtime
                    .set_connection(ConnectionState::Connected { generation });
                self.lifecycle.runtime.set_stream(StreamState::Negotiating);
            }
            SenderStatus::Streaming | SenderStatus::NetworkUnstable => {
                self.lifecycle
                    .runtime
                    .set_connection(ConnectionState::Connected { generation });
                self.lifecycle.runtime.set_trust(TrustState::Authenticated);
                self.lifecycle.runtime.set_stream(StreamState::Streaming {
                    generation: self.current_stream_epoch,
                });
                if status == SenderStatus::NetworkUnstable {
                    self.lifecycle
                        .runtime
                        .set_health(HealthState::NetworkDegraded);
                }
            }
            SenderStatus::PermissionRequired => self
                .lifecycle
                .runtime
                .set_output(OutputState::PermissionRequired),
        }
    }

    /// Close the active transport session (used by reconnect / recovery tests across crates).
    pub fn disconnect_for_test(&mut self, reason: picoo_transport::CloseReason) {
        if let Some(session) = self.active_session() {
            self.transport.close(session, reason);
        }
    }
}
