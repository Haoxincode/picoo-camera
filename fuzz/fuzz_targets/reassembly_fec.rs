#![no_main]

#[path = "../support.rs"]
mod support;

use std::time::{Duration, Instant};

use libfuzzer_sys::fuzz_target;
use picoo_packet::ReassemblyMap;
use picoo_protocol::VideoPacket;

fuzz_target!(|input: &[u8]| {
    let bytes = support::decode_seed(input);
    let origin = Instant::now();
    let mut reassembly = ReassemblyMap::new(8, 1_024);
    let mut cursor = 0_usize;
    let mut step = 0_u64;
    while cursor.saturating_add(2) <= bytes.len() && step < 2_048 {
        let length = u16::from_be_bytes([bytes[cursor], bytes[cursor + 1]]) as usize;
        cursor += 2;
        let end = cursor.saturating_add(length).min(bytes.len());
        if end == cursor {
            step += 1;
            continue;
        }
        if let Ok(packet) = VideoPacket::decode(&bytes[cursor..end]) {
            let now = origin + Duration::from_micros(step.saturating_mul(1_000));
            let _ = reassembly.ingest_at(packet, now);
            if bytes[cursor] & 0x20 != 0 {
                reassembly.expire_incomplete_older_than(now, Duration::from_millis(3));
            }
        }
        cursor = end;
        step += 1;
    }
    reassembly
        .expire_incomplete_older_than(origin + Duration::from_secs(10), Duration::from_millis(3));
    assert!(reassembly.oldest_unresolved_frame_id().is_none());
});
