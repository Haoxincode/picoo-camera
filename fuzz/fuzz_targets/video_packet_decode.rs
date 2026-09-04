#![no_main]

//! REQ-PICOO-PROTOCOL-007 — VideoPacket decode fuzz target.
//!
//! Run (nightly + cargo-fuzz):
//!   cargo install cargo-fuzz --version 0.13.2 --locked
//!   cargo +nightly fuzz run video-packet-decode

use libfuzzer_sys::fuzz_target;
use picoo_protocol::VideoPacket;

fuzz_target!(|data: &[u8]| {
    let _ = VideoPacket::decode(data);
});
