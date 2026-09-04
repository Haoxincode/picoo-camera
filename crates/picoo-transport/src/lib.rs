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

/// Platform-owned route constraint applied to each Sender UDP socket before Quinn takes it over.
///
/// Android's opaque value comes from `Network.getNetworkHandle()`. Apple interface indexes come
/// from Network.framework. The transport owns the socket operation so reconnects cannot silently
/// fall back to a VPN-selected default route (REQ-PICOO-DISCOVERY-007/008).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ClientNetworkBinding {
    #[default]
    Default,
    AndroidNetwork {
        network_handle: u64,
        /// A non-bypassable split-tunnel VPN can reject `android_setsocknetwork` even when its
        /// routing table explicitly leaves the Receiver's directly connected Wi-Fi subnet out of
        /// the tunnel. Android validates that narrow fallback before enabling it.
        allow_system_lan_route_fallback: bool,
    },
    AppleInterface(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionId(pub u64);

/// RFC 5705/8446 exporter bytes unique to one QUIC TLS connection.
pub type ChannelBinding = [u8; 32];

#[derive(Debug, Clone)]
pub enum TransportEvent {
    Connected(SessionId),
    ControlMessage(SessionId, Bytes),
    /// A short receive-side transport batch. Batching prevents one encoded
    /// keyframe from consuming hundreds of cross-thread event slots while the
    /// media layer still owns AU reassembly and deadline decisions.
    VideoPackets(SessionId, Vec<VideoPacket>),
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
    #[error("network binding failed: {0}")]
    NetworkBindingFailed(String),
    #[error("not connected")]
    NotConnected,
    /// The bounded media queue is full. Callers should drop this stale access
    /// unit and continue rather than treating the connection as failed.
    #[error("video queue is full")]
    VideoBackpressure,
    #[error("send failed: {0}")]
    SendFailed(String),
    #[error("TLS exporter channel binding is unavailable")]
    ChannelBindingUnavailable,
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
    /// Age of the last complete AU dequeued by the QUIC worker.
    pub video_queue_age_ms: f64,
    /// Complete AUs rejected by the application/Quinn datagram queues.
    pub video_dropped_access_units: u64,
    /// Bytes currently retained by Quinn's unreliable Datagram send buffer.
    pub video_buffered_bytes: u64,
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
    fn channel_binding(&self, session: SessionId) -> Result<ChannelBinding, TransportError>;

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
