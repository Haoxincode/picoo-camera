#![no_main]

//! REQ-PICOO-PROTOCOL-007 — VideoPacket decode fuzz target.
//!
//! Run (nightly + cargo-fuzz):
//!   cargo install cargo-fuzz
//!   cargo +nightly fuzz run video_packet_decode

use libfuzzer_sys::fuzz_target;
use picoo_protocol::VideoPacket;

fuzz_target!(|data: &[u8]| {
    let _ = VideoPacket::decode(data);
});
