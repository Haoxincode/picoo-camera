//! Sender session: packetization + transport flush.

use picoo_pairing::pairing_confirm_signature;
use picoo_protocol::control::{ClientHello, PairingChallenge, PairingConfirm, ServerHello};
use picoo_protocol::VideoPacket;
use picoo_protocol::ALPN;
use picoo_transport::{Endpoint, PicooTransport, SessionId, TransportEvent};
use prost::Message;

use crate::{SenderError, SenderPipeline, SenderStats};

#[derive(Debug, Clone)]
struct SenderPairing {
    receiver_id: String,
    challenge_nonce: Vec<u8>,
    short_code: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionStats {
    pub pipeline: SenderStats,
    pub sent_datagrams: u64,
}

pub struct SenderSession<T: PicooTransport> {
    pipeline: SenderPipeline,
    transport: T,
    session: Option<SessionId>,
    sent_datagrams: u64,
    pairing: Option<SenderPairing>,
    sender_id: Option<String>,
}

impl<T: PicooTransport> SenderSession<T> {
    pub fn new(transport: T) -> Self {
        Self {
            pipeline: SenderPipeline::default(),
            transport,
            session: None,
            sent_datagrams: 0,
            pairing: None,
            sender_id: None,
        }
    }

    fn drain_events(&mut self) {
        while let Some(event) = self.transport.poll_event() {
            match event {
                TransportEvent::Connected(session) => self.session = Some(session),
                TransportEvent::ControlMessage(_, msg) => self.handle_control(msg),
                TransportEvent::Disconnected(_, _) => {
                    self.session = None;
                    self.pairing = None;
                }
                TransportEvent::VideoPacket(_, _) => {}
            }
        }
    }

    fn handle_control(&mut self, msg: bytes::Bytes) {
        if let Ok(challenge) = PairingChallenge::decode(msg.as_ref()) {
            if let Some(pairing) = self.pairing.as_mut() {
                pairing.challenge_nonce = challenge.challenge_nonce;
                pairing.short_code = challenge.short_code;
            } else {
                self.pairing = Some(SenderPairing {
                    receiver_id: String::new(),
                    challenge_nonce: challenge.challenge_nonce,
                    short_code: challenge.short_code,
                });
            }
            return;
        }
        if let Ok(hello) = ServerHello::decode(msg.as_ref()) {
            if let Some(pairing) = self.pairing.as_mut() {
                pairing.receiver_id = hello.receiver_id;
            } else if hello.pairing_required {
                self.pairing = Some(SenderPairing {
                    receiver_id: hello.receiver_id,
                    challenge_nonce: Vec::new(),
                    short_code: String::new(),
                });
            }
        }
    }

    pub fn stats(&self) -> SessionStats {
        SessionStats {
            pipeline: self.pipeline.stats(),
            sent_datagrams: self.sent_datagrams,
        }
    }

    pub fn is_connected(&self) -> bool {
        self.session.is_some()
    }

    pub fn connect(&mut self, endpoint: Endpoint) -> Result<SessionId, SenderError> {
        let session = self
            .transport
            .connect(endpoint)
            .map_err(SenderError::Transport)?;
        self.drain_events();
        Ok(session)
    }

    pub fn pump(&mut self) -> Result<(), SenderError> {
        self.transport.pump().map_err(SenderError::Transport)?;
        self.drain_events();
        Ok(())
    }

    pub fn pairing_short_code(&self) -> Option<&str> {
        self.pairing
            .as_ref()
            .and_then(|p| (!p.short_code.is_empty()).then_some(p.short_code.as_str()))
    }

    pub fn ingest_access_unit(
        &mut self,
        data: &[u8],
        is_keyframe: bool,
        pts_us: u64,
        stream_epoch: u32,
    ) -> Result<usize, SenderError> {
        self.pipeline
            .ingest_access_unit(data, is_keyframe, pts_us, stream_epoch)
    }

    /// Send all pending VideoPackets over QUIC datagrams.
    pub fn flush_pending(&mut self) -> Result<usize, SenderError> {
        let session = self.session.ok_or(SenderError::NotConnected)?;
        let packets: Vec<VideoPacket> = self.pipeline.take_pending_packets();
        let mut sent = 0usize;
        for packet in packets {
            self.transport
                .send_video(session, packet)
                .map_err(SenderError::Transport)?;
            sent += 1;
        }
        self.transport.pump().map_err(SenderError::Transport)?;
        self.sent_datagrams += sent as u64;
        Ok(sent)
    }

    pub fn ingest_and_flush(
        &mut self,
        data: &[u8],
        is_keyframe: bool,
        pts_us: u64,
        stream_epoch: u32,
    ) -> Result<usize, SenderError> {
        self.ingest_access_unit(data, is_keyframe, pts_us, stream_epoch)?;
        self.flush_pending()
    }

    pub fn pending_packets(&self) -> usize {
        self.pipeline.pending_packets().len()
    }

    pub fn send_client_hello(
        &mut self,
        sender_id: &str,
        device_name: &str,
        public_key: &[u8],
    ) -> Result<(), SenderError> {
        let session = self.session.ok_or(SenderError::NotConnected)?;
        let hello = ClientHello {
            sender_id: sender_id.into(),
            device_name: device_name.into(),
            protocol_version: ALPN.into(),
            public_key: public_key.to_vec(),
        };
        self.sender_id = Some(sender_id.into());
        let mut buf = Vec::new();
        hello
            .encode(&mut buf)
            .map_err(|e| SenderError::Protocol(e.to_string()))?;
        self.transport
            .send_control(session, bytes::Bytes::from(buf))
            .map_err(SenderError::Transport)?;
        self.transport.pump().map_err(SenderError::Transport)?;
        self.drain_events();
        Ok(())
    }

    pub fn send_pairing_confirm(&mut self, receiver_id: &str) -> Result<(), SenderError> {
        let session = self.session.ok_or(SenderError::NotConnected)?;
        let pairing = self
            .pairing
            .as_ref()
            .ok_or_else(|| SenderError::Protocol("no pairing challenge".into()))?;
        let sender_id = self
            .sender_id
            .as_deref()
            .ok_or_else(|| SenderError::Protocol("missing sender id".into()))?;

        let confirm = PairingConfirm {
            confirm_signature: pairing_confirm_signature(
                &pairing.challenge_nonce,
                receiver_id,
                sender_id,
            ),
        };
        let mut buf = Vec::new();
        confirm
            .encode(&mut buf)
            .map_err(|e| SenderError::Protocol(e.to_string()))?;
        self.transport
            .send_control(session, bytes::Bytes::from(buf))
            .map_err(SenderError::Transport)?;
        self.transport.pump().map_err(SenderError::Transport)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use picoo_testkit::MemoryTransport;

    #[test]
    fn memory_transport_flush_pending() {
        let mut session = SenderSession::new(MemoryTransport::new());
        session
            .connect(Endpoint {
                host: "127.0.0.1".into(),
                port: 1,
            })
            .expect("connect");
        session
            .ingest_access_unit(b"au-bytes", true, 1, 1)
            .expect("ingest");
        let sent = session.flush_pending().expect("flush");
        assert_eq!(sent, 1);
        assert_eq!(session.stats().sent_datagrams, 1);
    }
}
