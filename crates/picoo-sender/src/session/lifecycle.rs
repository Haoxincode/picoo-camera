use std::time::{Duration, Instant};

use picoo_protocol::control::{
    control_envelope::Payload as ControlPayload, SenderStats as SenderStatsMsg, StartStream,
    StopStream,
};
use picoo_session::{ConnectionState, OutputState, StreamState};
use picoo_transport::{Endpoint, PicooTransport, SessionId, TransportEvent};

use super::reducer::{reduce, SenderEffect, SenderEffects, SenderEvent};
use super::SenderSession;
use crate::SenderError;

const SENDER_STATS_INTERVAL: Duration = Duration::from_secs(1);

impl<T: PicooTransport> SenderSession<T> {
    /// Surface camera/mic permission gate to UI (REQ-PICOO-SESSION-001).
    pub fn mark_permission_required(&mut self) {
        self.lifecycle
            .runtime
            .set_output(OutputState::PermissionRequired);
    }

    /// Clear permission gate once the host grants access (REQ-PICOO-SESSION-001).
    pub fn clear_permission_required(&mut self) {
        if self.lifecycle.runtime.output() == OutputState::PermissionRequired {
            self.lifecycle.runtime.set_output(OutputState::Ready);
        }
    }

    pub fn set_auto_reconnect(&mut self, enabled: bool) {
        if !enabled {
            self.reconnect_after = None;
        }
        let _ = self.apply_sender_event(SenderEvent::ReconnectPolicyChanged { enabled });
    }

    /// Delay scheduled by the most recent reconnect arming (REQ-PICOO-TRANSPORT-004).
    pub fn last_scheduled_reconnect_delay_ms(&self) -> Option<u64> {
        self.last_scheduled_reconnect_delay_ms
    }

    /// 1-based reconnect attempt while in [`picoo_session::SenderStatus::Reconnecting`].
    pub fn reconnect_attempt(&self) -> u32 {
        if matches!(
            self.lifecycle.runtime.connection(),
            ConnectionState::Reconnecting { .. }
        ) {
            self.reconnect_backoff.attempt()
        } else {
            0
        }
    }

    /// Sender → Receiver StartStream (PAIRING-003 / PROTOCOL control plane).
    pub fn send_start_stream(&mut self) -> Result<(), SenderError> {
        let session = self.active_session().ok_or(SenderError::NotConnected)?;
        self.send_control_payload(session, ControlPayload::StartStream(StartStream {}))?;
        self.drain_events();
        Ok(())
    }

    /// Sender → Receiver StopStream.
    pub fn send_stop_stream(&mut self) -> Result<(), SenderError> {
        let session = self.active_session().ok_or(SenderError::NotConnected)?;
        self.send_control_payload(session, ControlPayload::StopStream(StopStream {}))?;
        self.drain_events();
        Ok(())
    }

    pub(super) fn active_session(&self) -> Option<SessionId> {
        self.lifecycle.active_session()
    }

    fn reset_session_resources(&mut self) {
        self.abort_pending_reconfiguration();
        self.pairing = None;
        self.sender_nonce = None;
        self.stream_config_sent = false;
        self.clear_receiver_capabilities();
        self.pipeline.clear_pending_packets();
        self.last_sender_stats_sent_at = None;
        self.last_receiver_stats = None;
        self.pre_fec_packet_loss = 0.0;
        self.pending_camera_command = None;
        self.keyframe_requested = false;
        self.next_control_message_id = 1;
        self.last_received_control_message_id = 0;
        self.media_clock_anchor = None;
    }

    fn has_session_resources(&self) -> bool {
        self.encoder_apply_state.is_applying()
            || self.pairing.is_some()
            || self.sender_nonce.is_some()
            || self.stream_config_sent
            || self.receiver_capabilities.is_some()
            || self.pipeline.pending_datagram_count() != 0
            || self.pending_camera_command.is_some()
            || self.hello_requested
            || self.last_endpoint.is_some()
            || self.reconnect_after.is_some()
    }

    pub(super) fn apply_sender_event(
        &mut self,
        event: SenderEvent,
    ) -> Result<SenderEffects, SenderError> {
        let (state, effects) = reduce(self.lifecycle, event);
        self.lifecycle = state;
        for effect in effects.iter() {
            match *effect {
                SenderEffect::AcceptControl => {}
                SenderEffect::ResetSessionResources => self.reset_session_resources(),
                SenderEffect::CloseTransport { generation } => self.transport.close(
                    SessionId(generation),
                    picoo_transport::CloseReason::LocalClose,
                ),
                SenderEffect::PrepareConnection { generation } => {
                    debug_assert_eq!(self.active_session(), Some(SessionId(generation)));
                    self.reconnect_backoff.reset();
                    self.reconnect_after = None;
                    self.last_sender_stats_sent_at = None;
                    self.next_control_message_id = 1;
                    self.last_received_control_message_id = 0;
                    if self.hello_requested {
                        match self.emit_client_hello() {
                            Ok(()) => self.lifecycle.runtime.set_stream(StreamState::Negotiating),
                            Err(_) => self.reject_authentication("CLIENT_HELLO_SEND_FAILED"),
                        }
                    }
                }
                SenderEffect::ScheduleReconnect => {
                    if self.last_endpoint.is_none() {
                        continue;
                    }
                    let delay_ms = self.reconnect_backoff.next_delay_ms();
                    self.last_scheduled_reconnect_delay_ms = Some(delay_ms);
                    self.reconnect_after = Some(Instant::now() + Duration::from_millis(delay_ms));
                    self.apply_sender_event(SenderEvent::ReconnectArmed {
                        attempt: self.reconnect_backoff.attempt(),
                    })?;
                }
                SenderEffect::StartReconnect => {
                    let endpoint = self
                        .last_endpoint
                        .clone()
                        .ok_or(SenderError::NotConnected)?;
                    match self.transport.connect(endpoint) {
                        Ok(session) => {
                            self.apply_sender_event(SenderEvent::TransportConnectStarted {
                                generation: session.0,
                            })?;
                        }
                        Err(error) => {
                            self.apply_sender_event(SenderEvent::ReconnectConnectFailed)?;
                            return Err(SenderError::Transport(error));
                        }
                    }
                }
                SenderEffect::DisableReconnect => {
                    self.reconnect_after = None;
                }
                SenderEffect::ClearConnectionIntent => {
                    self.reconnect_after = None;
                    self.last_endpoint = None;
                    self.hello_requested = false;
                }
            }
        }
        Ok(effects)
    }

    pub(super) fn drain_events(&mut self) {
        while let Some(event) = self.transport.poll_event() {
            match event {
                TransportEvent::Connected(session) => {
                    let _ = self.apply_sender_event(SenderEvent::TransportConnected {
                        generation: session.0,
                    });
                }
                TransportEvent::ControlMessage(session, msg) => {
                    let effects = self.apply_sender_event(SenderEvent::ControlReceived {
                        generation: session.0,
                    });
                    if effects.is_ok_and(|effects| effects.contains(SenderEffect::AcceptControl)) {
                        self.handle_control(session, msg);
                    }
                }
                TransportEvent::Disconnected(session, _) => {
                    let _ = self.apply_sender_event(SenderEvent::TransportDisconnected {
                        generation: session.0,
                        endpoint_available: self.last_endpoint.is_some(),
                    });
                }
                TransportEvent::VideoPackets(_, _) => {}
            }
        }
    }

    pub fn connect(&mut self, endpoint: Endpoint) -> Result<SessionId, SenderError> {
        // Explicit connect re-enables automatic recovery after a user disconnect.
        self.reconnect_after = None;
        self.last_endpoint = Some(endpoint.clone());
        self.apply_sender_event(SenderEvent::ConnectRequested)?;
        let session = match self.transport.connect(endpoint) {
            Ok(session) => {
                self.apply_sender_event(SenderEvent::TransportConnectStarted {
                    generation: session.0,
                })?;
                session
            }
            Err(error) => {
                self.apply_sender_event(SenderEvent::ExplicitConnectFailed)?;
                return Err(SenderError::Transport(error));
            }
        };
        self.drain_events();
        Ok(session)
    }

    /// User-initiated stop: do not enter Reconnecting (PUC-005 live control).
    pub fn disconnect(&mut self) {
        let _ = self.apply_sender_event(SenderEvent::UserDisconnect {
            domain_resources_active: self.has_session_resources(),
        });
        // A local close may enqueue a Disconnected event. Its generation is
        // stale after the reducer transition and therefore cannot re-arm recovery.
        self.drain_events();
    }

    pub fn pump(&mut self) -> Result<(), SenderError> {
        self.drain_events();
        self.expire_encoder_transaction(Instant::now());
        if self
            .reconnect_after
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.reconnect_after = None;
            self.apply_sender_event(SenderEvent::ReconnectDeadlineElapsed)?;
            self.drain_events();
        }
        if self.lifecycle.runtime.stream().is_streaming() {
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
        let Some(session) = self.active_session() else {
            return;
        };
        let link = self.transport.link_stats().unwrap_or_default();
        let pipeline = self.pipeline.stats();
        let stats = SenderStatsMsg {
            access_units: pipeline.access_units,
            submitted_datagrams: self.sent_datagrams,
            video_queue_age_ms: link.video_queue_age_ms,
            video_dropped_access_units: link
                .video_dropped_access_units
                .saturating_add(pipeline.dropped_access_units),
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
        let _ = self.apply_sender_event(SenderEvent::ReconnectConnectFailed);
    }
}
