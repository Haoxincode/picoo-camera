//! Desktop-side QUIC listener actor.

use std::net::SocketAddr;
use std::str::FromStr;

use bytes::Bytes;

use crate::quinn_backend::{Command, TransportActor};
use crate::{CloseReason, Endpoint, SessionId, TransportError, TransportEvent, TransportLinkStats};

pub struct QuicReceiverTransport {
    actor: Option<TransportActor>,
}

impl Default for QuicReceiverTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl QuicReceiverTransport {
    pub fn new() -> Self {
        Self { actor: None }
    }

    pub fn bind_addr(&self) -> Option<SocketAddr> {
        self.actor
            .as_ref()
            .and_then(|actor| actor.local_addr().ok())
    }

    pub fn is_listening(&self) -> bool {
        self.actor.is_some()
    }

    pub fn is_connected(&self) -> bool {
        self.active_session().is_some()
    }

    pub fn active_session(&self) -> Option<SessionId> {
        self.actor.as_ref().and_then(TransportActor::active_session)
    }

    pub fn link_stats(&self) -> Option<TransportLinkStats> {
        self.actor.as_ref().and_then(TransportActor::link_stats)
    }

    pub fn bind(&mut self, endpoint: Endpoint) -> Result<SocketAddr, TransportError> {
        let addr = SocketAddr::from_str(&format!("{}:{}", endpoint.host, endpoint.port))
            .map_err(|error| TransportError::ConnectFailed(error.to_string()))?;
        let actor = TransportActor::server(addr)
            .map_err(|error| TransportError::ConnectFailed(error.to_string()))?;
        let local_addr = actor
            .local_addr()
            .map_err(|error| TransportError::ConnectFailed(error.to_string()))?;
        self.actor = Some(actor);
        Ok(local_addr)
    }

    pub fn poll_event(&mut self) -> Option<TransportEvent> {
        self.actor.as_ref()?.poll_event()
    }

    pub fn send_control(
        &mut self,
        session: SessionId,
        message: Bytes,
    ) -> Result<(), TransportError> {
        if self.active_session() != Some(session) {
            return Err(TransportError::NotConnected);
        }
        self.actor
            .as_ref()
            .ok_or(TransportError::NotConnected)?
            .command(Command::SendControl { session, message })
            .map_err(|error| TransportError::SendFailed(error.to_string()))
    }

    pub fn close(&mut self, session: SessionId, reason: CloseReason) {
        if let Some(actor) = &self.actor {
            let _ = actor.command(Command::Close { session, reason });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PicooTransport, QuicSenderTransport};
    use std::time::Duration;

    fn loopback() -> (QuicReceiverTransport, QuicSenderTransport, SessionId) {
        let mut receiver = QuicReceiverTransport::new();
        let addr = receiver
            .bind(Endpoint {
                host: "127.0.0.1".into(),
                port: 0,
            })
            .expect("bind");
        let mut sender = QuicSenderTransport::new();
        let pending = sender
            .connect(Endpoint {
                host: addr.ip().to_string(),
                port: addr.port(),
            })
            .expect("connect");

        let mut sender_connected = false;
        let mut receiver_connected = false;
        for _ in 0..500 {
            sender_connected |=
                matches!(sender.poll_event(), Some(TransportEvent::Connected(id)) if id == pending);
            receiver_connected |=
                matches!(receiver.poll_event(), Some(TransportEvent::Connected(_)));
            if sender_connected && receiver_connected {
                return (receiver, sender, pending);
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("loopback handshake timed out");
    }

    #[test]
    fn exchanges_control_messages_without_manual_io_pump() {
        let (mut receiver, mut sender, sender_session) = loopback();
        sender
            .send_control(sender_session, Bytes::from_static(b"hello"))
            .expect("send control");

        for _ in 0..200 {
            if matches!(
                receiver.poll_event(),
                Some(TransportEvent::ControlMessage(_, ref message)) if message.as_ref() == b"hello"
            ) {
                return;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("control message timed out");
    }
}
