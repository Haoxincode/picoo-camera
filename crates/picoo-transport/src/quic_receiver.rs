//! QUIC server-side transport for Windows/macOS receivers.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::str::FromStr;

use bytes::Bytes;
use picoo_protocol::VideoPacket;

use crate::quic::{QuicServer, QuicTransportError, CONTROL_STREAM_ID};
use crate::{CloseReason, Endpoint, SessionId, TransportError, TransportEvent};

pub struct QuicReceiverTransport {
    server: Option<QuicServer>,
    events: VecDeque<TransportEvent>,
    session: Option<SessionId>,
    connected_notified: bool,
}

impl Default for QuicReceiverTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl QuicReceiverTransport {
    pub fn new() -> Self {
        Self {
            server: None,
            events: VecDeque::new(),
            session: None,
            connected_notified: false,
        }
    }

    pub fn bind_addr(&self) -> Option<SocketAddr> {
        self.server.as_ref().and_then(|s| s.local_addr().ok())
    }

    pub fn is_listening(&self) -> bool {
        self.server.is_some()
    }

    pub fn is_connected(&self) -> bool {
        self.server
            .as_ref()
            .is_some_and(|server| server.is_established())
    }

    fn map_err(err: QuicTransportError) -> TransportError {
        TransportError::ConnectFailed(err.to_string())
    }

    fn map_send_err(err: QuicTransportError) -> TransportError {
        TransportError::SendFailed(err.to_string())
    }

    pub fn bind(&mut self, endpoint: Endpoint) -> Result<SocketAddr, TransportError> {
        let addr = SocketAddr::from_str(&format!("{}:{}", endpoint.host, endpoint.port))
            .map_err(|e| TransportError::ConnectFailed(e.to_string()))?;
        let server = QuicServer::bind(addr).map_err(Self::map_err)?;
        let local = server
            .local_addr()
            .map_err(|e| TransportError::ConnectFailed(e.to_string()))?;
        self.server = Some(server);
        Ok(local)
    }

    pub fn poll_event(&mut self) -> Option<TransportEvent> {
        self.events.pop_front()
    }

    pub fn send_control(
        &mut self,
        session: SessionId,
        message: Bytes,
    ) -> Result<(), TransportError> {
        if self.session != Some(session) {
            return Err(TransportError::NotConnected);
        }
        let server = self.server.as_mut().ok_or(TransportError::NotConnected)?;
        server
            .send_stream(CONTROL_STREAM_ID, &message)
            .map_err(Self::map_send_err)
    }

    pub fn close(&mut self, session: SessionId, reason: CloseReason) {
        if self.session == Some(session) {
            self.session = None;
            self.connected_notified = false;
            self.events
                .push_back(TransportEvent::Disconnected(session, reason));
        }
    }

    pub fn pump(&mut self) -> Result<(), TransportError> {
        let Some(server) = self.server.as_mut() else {
            return Ok(());
        };

        server.drive().map_err(Self::map_send_err)?;

        if server.is_established() && !self.connected_notified {
            let session = SessionId(1);
            self.session = Some(session);
            self.connected_notified = true;
            self.events.push_back(TransportEvent::Connected(session));
        }

        let session = match self.session {
            Some(s) => s,
            None => return Ok(()),
        };

        while let Some((stream_id, data)) = server.recv_stream().map_err(Self::map_send_err)? {
            if stream_id == CONTROL_STREAM_ID {
                self.events
                    .push_back(TransportEvent::ControlMessage(session, Bytes::from(data)));
            }
        }

        while let Some(raw) = server.recv_dgram().map_err(Self::map_send_err)? {
            if let Ok(packet) = VideoPacket::decode(&raw) {
                self.events
                    .push_back(TransportEvent::VideoPacket(session, packet));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use picoo_protocol::VideoPacketFlags;

    use crate::quic::establish_loopback;

    #[test]
    fn quic_receiver_transport_receives_video_datagram() {
        let pair = establish_loopback().expect("loopback");
        let mut server = pair.server;
        let mut client = pair.client;

        let mut receiver = QuicReceiverTransport::new();
        receiver.session = Some(SessionId(1));
        receiver.connected_notified = true;

        let packet = VideoPacket {
            version: VideoPacket::VERSION,
            flags: VideoPacketFlags::KEYFRAME,
            stream_epoch: 1,
            frame_id: 1,
            pts_us: 0,
            fragment_index: 0,
            fragment_count: 1,
            payload: Bytes::from_static(b"h264"),
        };
        client
            .send_dgram(&packet.encode().expect("encode"))
            .expect("send");
        client.drive().expect("client drive");
        server.drive().expect("server drive");

        receiver.server = Some(server);
        receiver.pump().expect("pump");
        let event = receiver.poll_event().expect("event");
        match event {
            TransportEvent::VideoPacket(_, pkt) => {
                assert_eq!(pkt.payload.as_ref(), b"h264");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }
}
