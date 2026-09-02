//! PCP/4 protocol — Protobuf control plane and FEC-protected VideoPacket.
//!
//! Requirement mapping: REQ-PICOO-PROTOCOL-*

pub mod control {
    include!(concat!(env!("OUT_DIR"), "/picoo.camera.v4.rs"));
}

mod video_fec;
mod video_packet;

pub use video_fec::{
    fec_group_ranges, make_fec_parity, reconstruct_fec_group, FecParityShard, FEC_DATA_SHARDS,
    FEC_PARITY_PREFIX_SIZE, FEC_PARITY_SHARDS,
};
pub use video_packet::{VideoPacket, VideoPacketError, VideoPacketFlags};

/// QUIC Application-Layer Protocol Negotiation identifier for PCP/4.
pub const ALPN: &str = "picoocam/4";

/// Maximum QUIC datagram size (path MTU safe).
pub const MAX_DATAGRAM_SIZE: usize = 1150;
/// Bounded H.264 access-unit size: about 1.1 MiB at the current datagram MTU.
/// This accommodates product-resolution IDRs while keeping reassembly memory finite.
pub const MAX_VIDEO_FRAGMENTS_PER_ACCESS_UNIT: u16 = 1024;

/// Fixed header size in bytes.
pub const VIDEO_PACKET_HEADER_SIZE: usize = 26;
/// Data fragments leave room for the FEC parity metadata prefix so parity and
/// data always fit the same path-MTU-safe QUIC Datagram size.
pub const MAX_FEC_FRAGMENT_PAYLOAD: usize =
    MAX_DATAGRAM_SIZE - VIDEO_PACKET_HEADER_SIZE - FEC_PARITY_PREFIX_SIZE;
