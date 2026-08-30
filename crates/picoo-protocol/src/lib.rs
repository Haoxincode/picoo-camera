//! PCP/2 protocol — Protobuf control plane and fixed-binary VideoPacket.
//!
//! Requirement mapping: REQ-PICOO-PROTOCOL-*

pub mod control {
    include!(concat!(env!("OUT_DIR"), "/picoo.camera.v2.rs"));
}

mod video_packet;

pub use video_packet::{VideoPacket, VideoPacketError, VideoPacketFlags};

/// QUIC Application-Layer Protocol Negotiation identifier for PCP/2.
pub const ALPN: &str = "picoocam/2";

/// Maximum QUIC datagram size (path MTU safe).
pub const MAX_DATAGRAM_SIZE: usize = 1150;
/// Bounded H.264 access-unit size: about 1.1 MiB at the current datagram MTU.
/// This accommodates product-resolution IDRs while keeping reassembly memory finite.
pub const MAX_VIDEO_FRAGMENTS_PER_ACCESS_UNIT: u16 = 1024;

/// Fixed header size in bytes.
pub const VIDEO_PACKET_HEADER_SIZE: usize = 26;
