use std::time::{Duration, Instant};

use picoo_protocol::control::{
    control_envelope::Payload as ControlPayload, SenderStats as SenderStatsMsg, StartStream,
    StopStream,
};
use picoo_session::{ConnectionState, OutputState, SessionRuntimeState, StreamState};
use picoo_transport::{Endpoint, PicooTransport, SessionId, TransportEvent};

use super::SenderSession;
use crate::SenderError;

const SENDER_STATS_INTERVAL: Duration = Duration::from_secs(1);

impl<T: PicooTransport> SenderSession<T> {
    /// Surface camera/mic permission gate to UI (REQ-PICOO-SESSION-001).
    pub fn mark_permission_required(&mut self) {
        self.runtime_state
            .set_output(OutputState::PermissionRequired);
    }

    /// Clear permission gate once the host grants access (REQ-PICOO-SESSION-001).
    pub fn clear_permission_required(&mut self) {
        if self.runtime_state.output() == OutputState::PermissionRequired {
            self.runtime_state.set_output(OutputState::Ready);
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
        if matches!(
            self.runtime_state.connection(),
            ConnectionState::Reconnecting { .. }
        ) {
            self.reconnect_backoff.attempt()
        } else {
            0
        }
    }

    /// Sender → Receiver StartStream (PAIRING-003 / PROTOCOL control plane).
    pub fn send_start_stream(&mut self) -> Result<(), SenderError> {
        let session = self.session.ok_or(SenderError::NotConnected)?;
        self.send_control_payload(session, ControlPayload::StartStream(StartStream {}))?;
        self.drain_events();
        Ok(())
    }

    /// Sender → Receiver StopStream.
    pub fn send_stop_stream(&mut self) -> Result<(), SenderError> {
        let session = self.session.ok_or(SenderError::NotConnected)?;
        self.send_control_payload(session, ControlPayload::StopStream(StopStream {}))?;
        self.drain_events();
        Ok(())
    }

    fn schedule_reconnect(&mut self) {
        if !self.auto_reconnect || self.last_endpoint.is_none() {
            self.runtime_state.reset_session(ConnectionState::Idle);
            return;
        }
        let delay_ms = self.reconnect_backoff.next_delay_ms();
        self.last_scheduled_reconnect_delay_ms = Some(delay_ms);
        self.reconnect_after = Some(Instant::now() + Duration::from_millis(delay_ms));
        self.runtime_state
            .reset_session(ConnectionState::Reconnecting {
                attempt: self.reconnect_backoff.attempt(),
            });
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
        self.runtime_state
            .reset_session(ConnectionState::Connected {
                generation: self.session.map_or(0, |session| session.0),
            });
        self.last_sender_stats_sent_at = None;
        self.next_control_message_id = 1;
        self.last_received_control_message_id = 0;
        if self.hello_requested {
            match self.emit_client_hello() {
                Ok(()) => self.runtime_state.set_stream(StreamState::Negotiating),
                Err(_) => self.reject_authentication("CLIENT_HELLO_SEND_FAILED"),
            }
        }
    }

    pub(super) fn drain_events(&mut self) {
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
                    self.sender_nonce = None;
                    self.stream_config_sent = false;
                    self.clear_receiver_capabilities();
                    self.pipeline.clear_pending_packets();
                    self.schedule_reconnect();
                }
                TransportEvent::VideoPackets(_, _) => {}
            }
        }
    }

    pub fn connect(&mut self, endpoint: Endpoint) -> Result<SessionId, SenderError> {
        // Explicit connect re-enables automatic recovery after a user disconnect.
        self.pipeline.clear_pending_packets();
        self.auto_reconnect = true;
        self.reconnect_after = None;
        self.last_endpoint = Some(endpoint.clone());
        self.runtime_state
            .reset_session(ConnectionState::Connecting);
        let session = match self.transport.connect(endpoint) {
            Ok(session) => session,
            Err(error) => {
                self.last_endpoint = None;
                self.runtime_state.reset_session(ConnectionState::Idle);
                return Err(SenderError::Transport(error));
            }
        };
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
        self.sender_nonce = None;
        self.hello_requested = false;
        self.pipeline.clear_pending_packets();
        self.abort_pending_reconfiguration();
        self.stream_config_sent = false;
        self.clear_receiver_capabilities();
        self.runtime_state = SessionRuntimeState::default();
    }

    pub fn pump(&mut self) -> Result<(), SenderError> {
        self.drain_events();
        self.expire_encoder_transaction(Instant::now());
        if matches!(
            self.runtime_state.connection(),
            ConnectionState::Reconnecting { .. }
        ) {
            self.try_reconnect()?;
            self.drain_events();
        }
        if self.runtime_state.stream().is_streaming() {
            self.send_pending_stream_config()?;
            self.maybe_send_sender_stats();
        }
        Ok(())
    }

    fn maybe_send_sender_stats(&mut self) {
        if self
            .last_sender_stats_sent_at
            .is_some_and(|last| last.elapsed() < SENDER_STATS_INTERVAL)
        {
            return;
        }
        let Some(session) = self.session else {
            return;
        };
        let link = self.transport.link_stats().unwrap_or_default();
        let pipeline = self.pipeline.stats();
        let stats = SenderStatsMsg {
            access_units: pipeline.access_units,
            submitted_datagrams: self.sent_datagrams,
            video_queue_age_ms: link.video_queue_age_ms,
            video_dropped_access_units: link.video_dropped_access_units,
            quic_lost_packets: link.lost_packets,
            quic_sent_packets: link.sent_packets,
            video_buffered_bytes: link.video_buffered_bytes,
        };
        if self
            .send_control_payload(session, ControlPayload::SenderStats(stats))
            .is_ok()
        {
            self.last_sender_stats_sent_at = Some(Instant::now());
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
