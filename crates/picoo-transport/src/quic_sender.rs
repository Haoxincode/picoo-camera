//! QUIC client implementing [`PicooTransport`] for Android/iOS senders.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::str::FromStr;

use bytes::Bytes;
use picoo_protocol::VideoPacket;

use crate::control_framing::{encode_control_frame, ControlFrameDecoder};
use crate::quic::{QuicClient, QuicTransportError, CONTROL_STREAM_ID};
use crate::{CloseReason, Endpoint, PicooTransport, SessionId, TransportError, TransportEvent};

pub struct QuicSenderTransport {
    client: Option<QuicClient>,
    session: Option<SessionId>,
    pending_session: Option<SessionId>,
    events: VecDeque<TransportEvent>,
    next_session: u64,
    control_rx: ControlFrameDecoder,
}

impl Default for QuicSenderTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl QuicSenderTransport {
    pub fn new() -> Self {
        Self {
            client: None,
            session: None,
            pending_session: None,
            events: VecDeque::new(),
            next_session: 1,
            control_rx: ControlFrameDecoder::default(),
        }
    }

    pub fn is_connected(&self) -> bool {
        self.client.as_ref().is_some_and(|c| c.is_established())
    }

    /// Wrap an already-established QUIC client (used in tests).
    pub fn from_established(client: QuicClient) -> Result<Self, TransportError> {
        if !client.is_established() {
            return Err(TransportError::ConnectFailed(
                "client not established".into(),
            ));
        }
        let session = SessionId(1);
        let mut events = VecDeque::new();
        events.push_back(TransportEvent::Connected(session));
        Ok(Self {
            client: Some(client),
            session: Some(session),
            pending_session: None,
            events,
            next_session: 2,
            control_rx: ControlFrameDecoder::default(),
        })
    }

    fn map_err(err: QuicTransportError) -> TransportError {
        TransportError::ConnectFailed(err.to_string())
    }

    fn map_send_err(err: QuicTransportError) -> TransportError {
        TransportError::SendFailed(err.to_string())
    }

    fn poll_inbound(&mut self) -> Result<(), TransportError> {
        let Some(client) = self.client.as_mut() else {
            return Ok(());
        };
        if !client.is_established() {
            return Ok(());
        }
        let session = self.session.expect("session set when established");

        while let Some((stream_id, data)) = client.recv_stream().map_err(Self::map_send_err)? {
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

        while let Some(raw) = client.recv_dgram().map_err(Self::map_send_err)? {
            if let Ok(packet) = VideoPacket::decode(&raw) {
                self.events
                    .push_back(TransportEvent::VideoPacket(session, packet));
            }
        }

        Ok(())
    }
}

impl PicooTransport for QuicSenderTransport {
    fn connect(&mut self, endpoint: Endpoint) -> Result<SessionId, TransportError> {
        if self.is_connected() {
            return Ok(self.session.expect("session when connected"));
        }

        if self.client.is_some() {
            return Ok(self
                .pending_session
                .expect("pending session while connecting"));
        }

        let addr = SocketAddr::from_str(&format!("{}:{}", endpoint.host, endpoint.port))
            .map_err(|e| TransportError::ConnectFailed(e.to_string()))?;

        let client = QuicClient::connect(addr).map_err(Self::map_err)?;
        let session = SessionId(self.next_session);
        self.next_session += 1;
        self.client = Some(client);
        self.pending_session = Some(session);
        self.pump()?;
        Ok(session)
    }

    fn send_control(&mut self, session: SessionId, message: Bytes) -> Result<(), TransportError> {
        if self.session != Some(session) {
            return Err(TransportError::NotConnected);
        }
        let client = self.client.as_mut().ok_or(TransportError::NotConnected)?;
        let framed = encode_control_frame(&message)
            .map_err(|e| TransportError::SendFailed(e.to_string()))?;
        client
            .send_stream(CONTROL_STREAM_ID, &framed)
            .map_err(Self::map_send_err)?;
        Ok(())
    }

    fn send_video(
        &mut self,
        session: SessionId,
        packet: VideoPacket,
    ) -> Result<(), TransportError> {
        if self.session != Some(session) {
            return Err(TransportError::NotConnected);
        }
        let client = self.client.as_mut().ok_or(TransportError::NotConnected)?;
        let encoded = packet
            .encode()
            .map_err(|e| TransportError::SendFailed(e.to_string()))?;
        client.send_dgram(&encoded).map_err(Self::map_send_err)?;
        Ok(())
    }

    fn poll_event(&mut self) -> Option<TransportEvent> {
        self.events.pop_front()
    }

    fn close(&mut self, session: SessionId, reason: CloseReason) {
        if self.session == Some(session) || self.pending_session == Some(session) {
            if let Some(mut client) = self.client.take() {
                let _ = client.close();
            }
            self.session = None;
            self.pending_session = None;
            self.control_rx = ControlFrameDecoder::default();
            self.events
                .push_back(TransportEvent::Disconnected(session, reason));
        }
    }

    fn pump(&mut self) -> Result<(), TransportError> {
        if let Some(client) = self.client.as_mut() {
            client.drive().map_err(Self::map_send_err)?;
        }

        if self.session.is_none() {
            if let Some(client) = self.client.as_ref() {
                if client.is_established() {
                    let session = self.pending_session.take().expect("pending session");
                    self.session = Some(session);
                    self.events.push_back(TransportEvent::Connected(session));
                }
            }
        }

        self.poll_inbound()?;
        Ok(())
    }

    fn link_stats(&self) -> Option<crate::TransportLinkStats> {
        self.client.as_ref().map(|c| c.link_stats())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use picoo_protocol::VideoPacketFlags;

    use crate::quic::establish_loopback;

    #[test]
    fn quic_sender_transport_sends_video_datagram() {
        let pair = establish_loopback().expect("loopback");
        let mut server = pair.server;
        let mut transport = QuicSenderTransport::from_established(pair.client).expect("wrap");
        let session = SessionId(1);

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
        transport.send_video(session, packet).expect("send");
        transport.pump().expect("pump");
        server.drive().expect("server drive");
        let raw = server.recv_dgram().expect("recv").expect("video");
        let decoded = VideoPacket::decode(&raw).expect("decode");
        assert_eq!(decoded.payload.as_ref(), b"h264");
    }
}
