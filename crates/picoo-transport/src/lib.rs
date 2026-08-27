//! Transport abstraction — business code must use this trait, not quiche directly.
//!
//! REQ-PICOO-TRANSPORT-001

use bytes::Bytes;
use picoo_protocol::VideoPacket;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionId(pub u64);

#[derive(Debug, Clone)]
pub enum TransportEvent {
    Connected(SessionId),
    ControlMessage(SessionId, Bytes),
    VideoPacket(SessionId, VideoPacket),
    Disconnected(SessionId, CloseReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseReason {
    LocalClose,
    PeerClose,
    Timeout,
    Error(String),
}

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("connection failed: {0}")]
    ConnectFailed(String),
    #[error("not connected")]
    NotConnected,
    #[error("send failed: {0}")]
    SendFailed(String),
}

pub trait PicooTransport {
    fn connect(&mut self, endpoint: Endpoint) -> Result<SessionId, TransportError>;
    fn send_control(&mut self, session: SessionId, message: Bytes) -> Result<(), TransportError>;
    fn send_video(&mut self, session: SessionId, packet: VideoPacket) -> Result<(), TransportError>;
    fn poll_event(&mut self) -> Option<TransportEvent>;
    fn close(&mut self, session: SessionId, reason: CloseReason);
}
