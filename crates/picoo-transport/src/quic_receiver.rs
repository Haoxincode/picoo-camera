//! QUIC server-side transport for Windows/macOS receivers.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::str::FromStr;

use bytes::Bytes;
use picoo_protocol::VideoPacket;

use crate::control_framing::{encode_control_frame, ControlFrameDecoder};
use crate::quic::{QuicServer, QuicTransportError, CONTROL_STREAM_ID};
use crate::{CloseReason, Endpoint, SessionId, TransportError, TransportEvent};

pub struct QuicReceiverTransport {
    server: Option<QuicServer>,
    events: VecDeque<TransportEvent>,
    session: Option<SessionId>,
    connected_notified: bool,
    control_rx: ControlFrameDecoder,
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
            control_rx: ControlFrameDecoder::default(),
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

    pub fn active_session(&self) -> Option<SessionId> {
        self.session
    }

    /// QUIC path/RTT/loss counters for ReceiverStats (REQ-PICOO-PROTOCOL-006).
    pub fn link_stats(&self) -> Option<crate::TransportLinkStats> {
        self.server.as_ref().and_then(|s| s.link_stats())
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
        let framed = encode_control_frame(&message)
            .map_err(|e| TransportError::SendFailed(e.to_string()))?;
        server
            .send_stream(CONTROL_STREAM_ID, &framed)
            .map_err(Self::map_send_err)
    }

    pub fn close(&mut self, session: SessionId, reason: CloseReason) {
        if self.session == Some(session) {
            self.session = None;
            self.connected_notified = false;
            self.control_rx = ControlFrameDecoder::default();
            self.events
                .push_back(TransportEvent::Disconnected(session, reason));
        }
    }

    pub fn pump(&mut self) -> Result<(), TransportError> {
        let Some(server) = self.server.as_mut() else {
            return Ok(());
        };

        server.drive().map_err(Self::map_send_err)?;
        let now_established = server.is_established();

        // Peer closed (or pruned): surface Disconnected so session layer can wait for reconnect.
        if self.connected_notified && !now_established {
            if let Some(session) = self.session.take() {
                self.connected_notified = false;
                self.control_rx = ControlFrameDecoder::default();
                self.events.push_back(TransportEvent::Disconnected(
                    session,
                    CloseReason::PeerClose,
                ));
            }
        }

        if now_established && !self.connected_notified {
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
                self.control_rx.push(&data);
                for message in self
                    .control_rx
                    .drain_messages()
                    .map_err(|e| TransportError::SendFailed(e.to_string()))?
                {
                    self.events
                        .push_back(TransportEvent::ControlMessage(session, message));
                }
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
    use crate::{Endpoint, PicooTransport, QuicSenderTransport};

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

    #[test]
    fn quic_receiver_gets_multiple_framed_control_messages() {
        let mut receiver = QuicReceiverTransport::new();
        let bind = receiver
            .bind(Endpoint {
                host: "127.0.0.1".into(),
                port: 0,
            })
            .expect("bind");

        let mut sender = QuicSenderTransport::new();
        let session = sender
            .connect(Endpoint {
                host: bind.ip().to_string(),
                port: bind.port(),
            })
            .expect("connect");

        for _ in 0..200 {
            receiver.pump().expect("receiver pump");
            sender.pump().expect("sender pump");
            if sender.is_connected() && receiver.is_connected() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }

        sender
            .send_control(session, Bytes::from_static(b"first"))
            .expect("first");
        sender
            .send_control(session, Bytes::from_static(b"second"))
            .expect("second");

        let mut seen = Vec::new();
        for _ in 0..100 {
            sender.pump().expect("sender pump");
            receiver.pump().expect("receiver pump");
            while let Some(event) = receiver.poll_event() {
                if let TransportEvent::ControlMessage(_, msg) = event {
                    seen.push(msg);
                }
            }
            if seen.len() >= 2 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }

        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].as_ref(), b"first");
        assert_eq!(seen[1].as_ref(), b"second");
    }

    #[test]
    fn quic_receiver_accepts_sender_reconnect_after_close() {
        // PUC-006 / REQ-PICOO-SESSION-008: peer close must free the listener for a new handshake.
        let mut receiver = QuicReceiverTransport::new();
        let bind = receiver
            .bind(Endpoint {
                host: "127.0.0.1".into(),
                port: 0,
            })
            .expect("bind");

        let mut sender = QuicSenderTransport::new();
        let session = sender
            .connect(Endpoint {
                host: bind.ip().to_string(),
                port: bind.port(),
            })
            .expect("connect");
        for _ in 0..200 {
            receiver.pump().ok();
            sender.pump().ok();
            if sender.is_connected() && receiver.is_connected() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(receiver.is_connected());

        sender.close(session, CloseReason::Timeout);
        // Drain sender Disconnected, then drive both until receiver observes peer close.
        let _ = sender.poll_event();
        let mut saw_disconnect = false;
        for _ in 0..200 {
            receiver.pump().ok();
            sender.pump().ok();
            while let Some(event) = receiver.poll_event() {
                if matches!(event, TransportEvent::Disconnected(_, _)) {
                    saw_disconnect = true;
                }
            }
            if saw_disconnect && !receiver.is_connected() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(saw_disconnect, "receiver should observe peer disconnect");

        let session2 = sender
            .connect(Endpoint {
                host: bind.ip().to_string(),
                port: bind.port(),
            })
            .expect("reconnect");
        let mut saw_connect = false;
        for _ in 0..400 {
            receiver.pump().ok();
            sender.pump().ok();
            while let Some(event) = receiver.poll_event() {
                if matches!(event, TransportEvent::Connected(_)) {
                    saw_connect = true;
                }
            }
            if sender.is_connected() && receiver.is_connected() && saw_connect {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(sender.is_connected(), "sender reconnect established");
        assert!(receiver.is_connected(), "receiver accepted reconnect");
        assert!(saw_connect, "receiver emitted Connected for reconnect");
        assert_ne!(session.0, session2.0);
    }
}
