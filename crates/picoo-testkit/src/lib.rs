//! Protocol simulation over real QUIC loopback — Goal step 2.

mod memory;
mod quic_sim;

pub use memory::MemoryTransport;
pub use quic_sim::{run_quic_protocol_simulation, QuicSimulationError};

use bytes::Bytes;
use picoo_protocol::VideoPacket;

/// Simulate fragment send/receive through reassembly.
pub fn simulate_video_roundtrip(packets: Vec<VideoPacket>) -> Option<Bytes> {
    use picoo_packet::ReassemblyMap;
    let mut map = ReassemblyMap::new(16, 32);
    let mut last = None;
    for packet in packets {
        last = map
            .ingest(packet)
            .ok()
            .flatten()
            .map(|au| au.data)
            .or(last);
    }
    last
}
