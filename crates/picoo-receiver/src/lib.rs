//! Desktop receiver session: QUIC ingress → reassembly → FrameHub.
//!
//! REQ-PICOO-FRAME-001, REQ-PICOO-MEDIA-005/006 (decode placeholder until MF/VT).

use std::time::Duration;

use bytes::Bytes;
use picoo_frame_hub::{FrameHub, FrameSlot};
use picoo_packet::ReassemblyMap;
use picoo_session::ReceiverStatus;
use picoo_transport::{
    CloseReason, Endpoint, QuicReceiverTransport, TransportError, TransportEvent,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ReceiverError {
    #[error("transport: {0}")]
    Transport(#[from] TransportError),
    #[error("sender: {0}")]
    Sender(#[from] picoo_sender::SenderError),
    #[error("frame hub: {0}")]
    FrameHub(#[from] picoo_frame_hub::FrameHubError),
    #[error("not listening")]
    NotListening,
    #[error("loopback timeout")]
    LoopbackTimeout,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReceiverStats {
    pub access_units: u64,
    pub packets_received: u64,
}

pub struct ReceiverSession {
    transport: QuicReceiverTransport,
    reassembly: ReassemblyMap,
    frame_hub: FrameHub,
    status: ReceiverStatus,
    stats: ReceiverStats,
}

impl Default for ReceiverSession {
    fn default() -> Self {
        Self::new()
    }
}

impl ReceiverSession {
    pub fn new() -> Self {
        Self {
            transport: QuicReceiverTransport::new(),
            reassembly: ReassemblyMap::new(8, 16),
            frame_hub: FrameHub::new(),
            status: ReceiverStatus::Disconnected,
            stats: ReceiverStats::default(),
        }
    }

    pub fn status(&self) -> ReceiverStatus {
        self.status
    }

    pub fn stats(&self) -> ReceiverStats {
        self.stats
    }

    pub fn is_connected(&self) -> bool {
        self.transport.is_connected()
    }

    pub fn frame_hub(&self) -> &FrameHub {
        &self.frame_hub
    }

    pub fn bind_addr(&self) -> Option<std::net::SocketAddr> {
        self.transport.bind_addr()
    }

    pub fn listen(&mut self, endpoint: Endpoint) -> Result<std::net::SocketAddr, ReceiverError> {
        let addr = self.transport.bind(endpoint)?;
        self.status = ReceiverStatus::Discovering;
        Ok(addr)
    }

    pub fn pump(&mut self) -> Result<(), ReceiverError> {
        self.transport.pump()?;

        while let Some(event) = self.transport.poll_event() {
            match event {
                TransportEvent::Connected(_) => {
                    self.status = ReceiverStatus::Streaming;
                }
                TransportEvent::Disconnected(_, _) => {
                    self.status = ReceiverStatus::Disconnected;
                    self.reassembly = ReassemblyMap::new(8, 16);
                }
                TransportEvent::ControlMessage(_, _msg) => {
                    // ClientHello/ServerHello handled in pairing step.
                }
                TransportEvent::VideoPacket(_, packet) => {
                    self.stats.packets_received += 1;
                    if let Some(access_unit) = self.reassembly.ingest(packet).ok().flatten() {
                        self.publish_access_unit(access_unit)?;
                    }
                }
            }
        }

        Ok(())
    }

    pub fn close(&mut self) {
        if self.transport.is_connected() {
            self.transport
                .close(picoo_transport::SessionId(1), CloseReason::LocalClose);
        }
        self.status = ReceiverStatus::Disconnected;
    }

    /// Placeholder decode path: store H.264 access unit bytes until MF/VT decoder lands.
    fn publish_access_unit(&mut self, access_unit: Bytes) -> Result<(), ReceiverError> {
        self.stats.access_units += 1;
        let index = self.frame_hub.begin_write()?;
        self.frame_hub
            .commit_write(index, 1280, 720, 1280, 0, 0, access_unit);
        Ok(())
    }

    pub fn latest_frame(&self) -> Option<&FrameSlot> {
        self.frame_hub.latest_ready()
    }
}

/// Run sender→receiver loopback until one access unit reaches FrameHub.
pub fn run_loopback_access_unit(payload: &[u8]) -> Result<Bytes, ReceiverError> {
    use picoo_sender::SenderSession;
    use picoo_transport::{Endpoint, QuicSenderTransport};

    let mut receiver = ReceiverSession::new();
    let bind = receiver.listen(Endpoint {
        host: "127.0.0.1".into(),
        port: 0,
    })?;

    let mut sender = SenderSession::new(QuicSenderTransport::new());
    let endpoint = Endpoint {
        host: bind.ip().to_string(),
        port: bind.port(),
    };
    sender.connect(endpoint)?;

    for _ in 0..500 {
        receiver.pump()?;
        sender.pump()?;
        if sender.is_connected() && receiver.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    if !receiver.is_connected() {
        return Err(ReceiverError::LoopbackTimeout);
    }

    sender.ingest_and_flush(payload, true, 1, 1)?;

    for _ in 0..200 {
        receiver.pump()?;
        sender.pump().ok();
        if let Some(frame) = receiver.latest_frame() {
            return Ok(frame.pixel_data.clone());
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    Err(ReceiverError::LoopbackTimeout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_sender_to_receiver_frame_hub() {
        let frame = run_loopback_access_unit(b"test-access-unit").expect("loopback");
        assert_eq!(frame.as_ref(), b"test-access-unit");
    }
}
