use std::time::{Duration, Instant};

use picoo_protocol::control::{SenderStats as SenderStatsMsg, StartStream, StopStream};
use picoo_session::SenderStatus;
use picoo_transport::{Endpoint, PicooTransport, SessionId, TransportEvent};
use prost::Message;

use super::SenderSession;
use crate::SenderError;

const SENDER_STATS_MAGIC: u32 = 0x5354_4154;
const SENDER_STATS_INTERVAL: Duration = Duration::from_secs(1);

impl<T: PicooTransport> SenderSession<T> {
    /// Surface camera/mic permission gate to UI (REQ-PICOO-SESSION-001).
    pub fn mark_permission_required(&mut self) {
        if self.status != SenderStatus::PermissionRequired {
            self.permission_resume_status = Some(self.status);
        }
        self.status = SenderStatus::PermissionRequired;
    }

    /// Clear permission gate once the host grants access (REQ-PICOO-SESSION-001).
    pub fn clear_permission_required(&mut self) {
        if self.status == SenderStatus::PermissionRequired {
            self.status = self
                .permission_resume_status
                .take()
                .unwrap_or(SenderStatus::Disconnected);
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
        self.last_sender_stats_sent_at = None;
        if let Some(params) = self.hello_params.clone() {
            if self.emit_client_hello(&params).is_ok() {
                self.status = SenderStatus::Negotiating;
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
            magic: SENDER_STATS_MAGIC,
            access_units: pipeline.access_units,
            submitted_datagrams: self.sent_datagrams,
            video_queue_age_ms: link.video_queue_age_ms,
            video_dropped_access_units: link.video_dropped_access_units,
            quic_lost_packets: link.lost_packets,
            quic_sent_packets: link.sent_packets,
            video_buffered_bytes: link.video_buffered_bytes,
        };
        let mut bytes = Vec::new();
        if stats.encode(&mut bytes).is_ok()
            && self
                .transport
                .send_control(session, bytes::Bytes::from(bytes))
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
