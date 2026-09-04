//! Desktop-side QUIC listener actor.

use std::net::SocketAddr;
use std::str::FromStr;

use bytes::Bytes;

use crate::quinn_backend::{Command, TransportActor};
use crate::{
    ChannelBinding, CloseReason, Endpoint, SessionId, TransportError, TransportEvent,
    TransportLinkStats,
};

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

    pub fn close_active(&mut self, reason: CloseReason) {
        if let Some(session) = self.active_session() {
            self.close(session, reason);
        }
    }

    pub fn channel_binding(&self, session: SessionId) -> Result<ChannelBinding, TransportError> {
        self.actor
            .as_ref()
            .and_then(|actor| actor.channel_binding(session))
            .ok_or(TransportError::ChannelBindingUnavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PicooTransport, QuicSenderTransport};
    use std::time::Duration;

    fn loopback() -> (
        QuicReceiverTransport,
        QuicSenderTransport,
        SessionId,
        SessionId,
    ) {
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
                let receiver_session = receiver.active_session().expect("receiver session");
                return (receiver, sender, receiver_session, pending);
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("loopback handshake timed out");
    }

    #[test]
    fn exchanges_control_messages_without_manual_io_pump() {
        let (mut receiver, mut sender, _, sender_session) = loopback();
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

    #[test]
    fn sender_event_wake_observes_control_before_platform_poll() {
        let (mut receiver, mut sender, receiver_session, _) = loopback();
        let wake = sender.event_wake();
        let observed = wake.revision();
        receiver
            .send_control(receiver_session, Bytes::from_static(b"wake"))
            .expect("send control");

        let changed = wake.wait_after(observed, Duration::from_secs(1));
        assert!(changed > observed);
        assert!(matches!(
            sender.poll_event(),
            Some(TransportEvent::ControlMessage(_, ref message)) if message.as_ref() == b"wake"
        ));
        assert_eq!(
            wake.wait_after(observed, Duration::ZERO),
            changed,
            "a concurrent Session pump cannot erase the platform wake revision"
        );
    }

    #[test]
    fn close_active_targets_a_later_connection_generation() {
        let (mut receiver, mut first_sender, first_receiver_session, _) = loopback();
        let addr = receiver.bind_addr().expect("receiver address");
        receiver.close_active(CloseReason::LocalClose);
        for _ in 0..200 {
            let _ = receiver.poll_event();
            if matches!(
                first_sender.poll_event(),
                Some(TransportEvent::Disconnected(_, _))
            ) {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }

        let mut second_sender = QuicSenderTransport::new();
        let second_pending = second_sender
            .connect(Endpoint {
                host: addr.ip().to_string(),
                port: addr.port(),
            })
            .expect("second connect");
        let mut second_receiver_session = None;
        for _ in 0..500 {
            let _ = second_sender.poll_event();
            if let Some(TransportEvent::Connected(session)) = receiver.poll_event() {
                second_receiver_session = Some(session);
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        let second_receiver_session = second_receiver_session.expect("second receiver session");
        assert_ne!(second_receiver_session, first_receiver_session);
        assert!(second_pending.0 > 0);
        assert!(second_sender.is_connected());

        receiver.close_active(CloseReason::LocalClose);
        for _ in 0..200 {
            let _ = receiver.poll_event();
            if matches!(
                second_sender.poll_event(),
                Some(TransportEvent::Disconnected(_, _))
            ) {
                assert!(receiver.active_session().is_none());
                return;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("second connection was not closed");
    }
}
