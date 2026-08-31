//! Transport abstraction — business code must use domain messages, not Quinn directly.
//!
//! REQ-PICOO-TRANSPORT-001

mod control_framing;
mod quinn_backend;
mod receiver;
mod sender;

use bytes::Bytes;
use picoo_protocol::VideoPacket;
use thiserror::Error;

pub use control_framing::{
    encode_control_frame, ControlFrameDecoder, ControlFramingError, MAX_CONTROL_MESSAGE_SIZE,
};

pub use quinn_backend::QuicTransportError;
pub use receiver::QuicReceiverTransport;
pub use sender::QuicSenderTransport;

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

/// Path/connection counters for ReceiverStats (REQ-PICOO-PROTOCOL-006).
///
/// Opaque to business code — never exposes Quinn types (ARCH-PICOO-TRANSPORT-001).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TransportLinkStats {
    pub rtt_ms: f64,
    pub lost_packets: u64,
    pub sent_packets: u64,
    pub recv_packets: u64,
    pub dgram_recv: u64,
}

impl TransportLinkStats {
    /// QUIC-reported loss ratio on packets this endpoint sent (`lost / sent`).
    pub fn sent_loss_ratio(&self) -> f64 {
        if self.sent_packets == 0 {
            0.0
        } else {
            self.lost_packets as f64 / self.sent_packets as f64
        }
    }
}

pub trait PicooTransport {
    fn connect(&mut self, endpoint: Endpoint) -> Result<SessionId, TransportError>;
    fn send_control(&mut self, session: SessionId, message: Bytes) -> Result<(), TransportError>;
    fn send_video(&mut self, session: SessionId, packet: VideoPacket)
        -> Result<(), TransportError>;
    /// Queue one encoded access unit atomically. Implementations with a lossy
    /// transport must drop the whole batch rather than leaving a partial frame.
    fn send_video_batch(
        &mut self,
        session: SessionId,
        packets: Vec<VideoPacket>,
    ) -> Result<(), TransportError> {
        for packet in packets {
            self.send_video(session, packet)?;
        }
        Ok(())
    }
    fn poll_event(&mut self) -> Option<TransportEvent>;
    fn close(&mut self, session: SessionId, reason: CloseReason);

    /// Optional QUIC path stats for ReceiverStats / ABR (REQ-PICOO-PROTOCOL-006).
    fn link_stats(&self) -> Option<TransportLinkStats> {
        None
    }
}

#[cfg(test)]
mod link_stats_tests {
    use super::*;

    #[test]
    fn sent_loss_ratio_handles_empty_and_partial() {
        assert_eq!(TransportLinkStats::default().sent_loss_ratio(), 0.0);
        let stats = TransportLinkStats {
            lost_packets: 5,
            sent_packets: 100,
            ..Default::default()
        };
        assert!((stats.sent_loss_ratio() - 0.05).abs() < f64::EPSILON);
    }
}
