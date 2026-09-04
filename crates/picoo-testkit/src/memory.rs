//! In-memory transport for unit tests without network I/O.

use std::collections::VecDeque;

use bytes::Bytes;
use picoo_transport::{
    ChannelBinding, CloseReason, Endpoint, PicooTransport, SessionId, TransportError,
    TransportEvent, VideoDatagramBatch,
};

pub struct MemoryTransport {
    next_session: u64,
    events: VecDeque<TransportEvent>,
    connected: Option<SessionId>,
}

impl Default for MemoryTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryTransport {
    pub fn new() -> Self {
        Self {
            next_session: 1,
            events: VecDeque::new(),
            connected: None,
        }
    }

    pub fn push_event(&mut self, event: TransportEvent) {
        self.events.push_back(event);
    }
}

impl PicooTransport for MemoryTransport {
    fn connect(&mut self, _endpoint: Endpoint) -> Result<SessionId, TransportError> {
        let id = SessionId(self.next_session);
        self.next_session += 1;
        self.connected = Some(id);
        self.events.push_back(TransportEvent::Connected(id));
        Ok(id)
    }

    fn send_control(&mut self, session: SessionId, message: Bytes) -> Result<(), TransportError> {
        if self.connected != Some(session) {
            return Err(TransportError::NotConnected);
        }
        self.events
            .push_back(TransportEvent::ControlMessage(session, message));
        Ok(())
    }

    fn send_video_batch(
        &mut self,
        session: SessionId,
        batch: VideoDatagramBatch,
    ) -> Result<(), TransportError> {
        if self.connected != Some(session) {
            return Err(TransportError::NotConnected);
        }
        let packets = batch
            .into_datagrams()
            .into_iter()
            .filter_map(|datagram| picoo_protocol::VideoPacket::decode_bytes(datagram).ok())
            .collect::<Vec<_>>();
        if !packets.is_empty() {
            self.events
                .push_back(TransportEvent::VideoPackets(session, packets));
        }
        Ok(())
    }

    fn poll_event(&mut self) -> Option<TransportEvent> {
        self.events.pop_front()
    }

    fn close(&mut self, session: SessionId, reason: CloseReason) {
        if self.connected == Some(session) {
            self.connected = None;
        }
        self.events
            .push_back(TransportEvent::Disconnected(session, reason));
    }

    fn channel_binding(&self, session: SessionId) -> Result<ChannelBinding, TransportError> {
        if self.connected == Some(session) {
            Ok([0x42; 32])
        } else {
            Err(TransportError::ChannelBindingUnavailable)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use picoo_protocol::{VideoPacket, VideoPacketFlags};

    #[test]
    fn memory_transport_connect_and_send() {
        let mut transport = MemoryTransport::new();
        let session = transport
            .connect(Endpoint {
                host: "127.0.0.1".into(),
                port: 4433,
            })
            .unwrap();
        transport
            .send_control(session, Bytes::from_static(b"hello"))
            .unwrap();
        assert!(matches!(
            transport.poll_event(),
            Some(TransportEvent::Connected(_))
        ));
        assert!(matches!(
            transport.poll_event(),
            Some(TransportEvent::ControlMessage(_, _))
        ));
    }

    #[test]
    fn simulate_two_fragment_frame() {
        use bytes::Bytes;
        let packets = vec![
            VideoPacket {
                flags: VideoPacketFlags::empty(),
                stream_epoch: 1,
                frame_id: 1,
                pts_us: 0,
                encoded_at_us: 0,
                fragment_index: 0,
                fragment_count: 2,
                payload: Bytes::from_static(b"aa"),
            },
            VideoPacket {
                flags: VideoPacketFlags::empty(),
                stream_epoch: 1,
                frame_id: 1,
                pts_us: 0,
                encoded_at_us: 0,
                fragment_index: 1,
                fragment_count: 2,
                payload: Bytes::from_static(b"bb"),
            },
        ];
        assert_eq!(
            crate::simulate_video_roundtrip(packets).as_deref(),
            Some(&b"aabb"[..])
        );
    }
}
