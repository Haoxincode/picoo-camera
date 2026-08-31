use picoo_protocol::control::{camera_command, encoder_command, CameraCommand, EncoderCommand};
use picoo_protocol::VideoPacket;
use picoo_session::SenderStatus;
use picoo_transport::{PicooTransport, SessionId};

use super::SenderSession;
use crate::SenderError;

impl<T: PicooTransport> SenderSession<T> {
    pub(super) fn enter_streaming(&mut self) {
        self.status = SenderStatus::Streaming;
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
        let sent = packets.len();
        self.transport
            .send_video_batch(session, packets)
            .map_err(SenderError::Transport)?;
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
}
