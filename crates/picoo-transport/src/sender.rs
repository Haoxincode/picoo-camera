//! Mobile-side QUIC transport actor.

use std::net::SocketAddr;
use std::str::FromStr;

use bytes::Bytes;
use picoo_protocol::VideoPacket;

use crate::quinn_backend::{Command, QuicTransportError, TransportActor};
use crate::{
    CloseReason, Endpoint, PicooTransport, SessionId, TransportError, TransportEvent,
    TransportLinkStats,
};

pub struct QuicSenderTransport {
    actor: Option<TransportActor>,
    pending_session: Option<SessionId>,
    active_session: Option<SessionId>,
    next_session: u64,
}

impl Default for QuicSenderTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl QuicSenderTransport {
    pub fn new() -> Self {
        Self {
            actor: None,
            pending_session: None,
            active_session: None,
            next_session: 1,
        }
    }

    pub fn is_connected(&self) -> bool {
        self.active_session.is_some()
            && self.actor.as_ref().and_then(TransportActor::active_session) == self.active_session
    }

    fn map_connect_error(error: impl ToString) -> TransportError {
        TransportError::ConnectFailed(error.to_string())
    }

    fn map_send_error(error: QuicTransportError) -> TransportError {
        match error {
            QuicTransportError::VideoBackpressure => TransportError::VideoBackpressure,
            error => TransportError::SendFailed(error.to_string()),
        }
    }
}

impl PicooTransport for QuicSenderTransport {
    fn connect(&mut self, endpoint: Endpoint) -> Result<SessionId, TransportError> {
        if let Some(session) = self.active_session.or(self.pending_session) {
            return Ok(session);
        }

        let server_addr = SocketAddr::from_str(&format!("{}:{}", endpoint.host, endpoint.port))
            .map_err(Self::map_connect_error)?;
        let actor = TransportActor::client(server_addr).map_err(Self::map_connect_error)?;
        let session = SessionId(self.next_session);
        self.next_session += 1;
        actor
            .command(Command::Connect {
                session,
                server_addr,
            })
            .map_err(Self::map_connect_error)?;
        self.actor = Some(actor);
        self.pending_session = Some(session);
        Ok(session)
    }

    fn send_control(&mut self, session: SessionId, message: Bytes) -> Result<(), TransportError> {
        if self.active_session != Some(session) {
            return Err(TransportError::NotConnected);
        }
        self.actor
            .as_ref()
            .ok_or(TransportError::NotConnected)?
            .command(Command::SendControl { session, message })
            .map_err(Self::map_send_error)
    }

    fn send_video(
        &mut self,
        session: SessionId,
        packet: VideoPacket,
    ) -> Result<(), TransportError> {
        if self.active_session != Some(session) {
            return Err(TransportError::NotConnected);
        }
        self.actor
            .as_ref()
            .ok_or(TransportError::NotConnected)?
            .send_video_batch(session, vec![packet])
            .map_err(Self::map_send_error)
    }

    fn send_video_batch(
        &mut self,
        session: SessionId,
        packets: Vec<VideoPacket>,
    ) -> Result<(), TransportError> {
        if self.active_session != Some(session) {
            return Err(TransportError::NotConnected);
        }
        self.actor
            .as_ref()
            .ok_or(TransportError::NotConnected)?
            .send_video_batch(session, packets)
            .map_err(Self::map_send_error)
    }

    fn poll_event(&mut self) -> Option<TransportEvent> {
        let event = self.actor.as_ref()?.poll_event()?;
        match &event {
            TransportEvent::Connected(session) => {
                self.pending_session = None;
                self.active_session = Some(*session);
            }
            TransportEvent::Disconnected(session, _) => {
                if self.active_session == Some(*session) {
                    self.active_session = None;
                }
                if self.pending_session == Some(*session) {
                    self.pending_session = None;
                }
            }
            _ => {}
        }
        Some(event)
    }

    fn close(&mut self, session: SessionId, reason: CloseReason) {
        if self.active_session == Some(session) || self.pending_session == Some(session) {
            if let Some(actor) = &self.actor {
                let _ = actor.command(Command::Close { session, reason });
            }
        }
    }

    fn link_stats(&self) -> Option<TransportLinkStats> {
        self.actor.as_ref().and_then(TransportActor::link_stats)
    }
}
