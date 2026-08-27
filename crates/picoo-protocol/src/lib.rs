//! PCP/1 protocol — Protobuf control plane and fixed-binary VideoPacket.
//!
//! Requirement mapping: REQ-PICOO-PROTOCOL-*

pub mod control {
    include!(concat!(env!("OUT_DIR"), "/picoo.camera.v1.rs"));
}

mod video_packet;

pub use video_packet::{VideoPacket, VideoPacketError, VideoPacketFlags};

/// QUIC Application-Layer Protocol Negotiation identifier for PCP/1.
pub const ALPN: &str = "picoocam/1";

/// Maximum QUIC datagram size (path MTU safe).
pub const MAX_DATAGRAM_SIZE: usize = 1150;

/// Fixed header size in bytes.
pub const VIDEO_PACKET_HEADER_SIZE: usize = 26;
