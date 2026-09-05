use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;
use picoo_packet::ReassemblyMap;
use picoo_protocol::{
    fec_group_for_fragment, fec_group_ranges, make_fec_parity_count, VideoPacket, VideoPacketFlags,
};

const SAMPLE_TIME: Duration = Duration::from_millis(300);

struct CountingAllocator;
static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);

// SAFETY: allocation operations and their original pointer/layout pairs are
// forwarded unchanged to the system allocator.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn main() {
    for fragment_count in [12, 64, 256] {
        run(fragment_count, 0, false, "clean");
        run(fragment_count, 0, true, "reverse");
        run(fragment_count, 1, true, "light-loss");
        run(fragment_count, 2, true, "strong-loss");
    }
}

fn run(fragment_count: u16, parity_count: usize, reverse: bool, label: &str) {
    let packets = packets(fragment_count, parity_count, reverse);
    ALLOCATIONS.store(0, Ordering::Relaxed);
    let started = Instant::now();
    let mut frames = 0_u64;
    let mut checks = 0_u64;
    let mut attempts = 0_u64;
    while started.elapsed() < SAMPLE_TIME {
        let mut reassembly = ReassemblyMap::new(8, 1_024);
        for packet in packets.iter().cloned() {
            black_box(reassembly.ingest(packet).expect("benchmark packet"));
        }
        checks += reassembly.fec_group_check_count();
        attempts += reassembly.fec_recovery_attempt_count();
        frames += 1;
    }
    let elapsed = started.elapsed().as_secs_f64();
    let allocations = ALLOCATIONS.load(Ordering::Relaxed);
    println!(
        "{fragment_count:>3} fragments {label:<11}: {:>8.1} frames/s, {:>7.1} allocations/frame, {:>6.1} group checks/frame, {:>5.1} recoveries/frame",
        frames as f64 / elapsed,
        allocations as f64 / frames as f64,
        checks as f64 / frames as f64,
        attempts as f64 / frames as f64,
    );
}

fn packets(fragment_count: u16, parity_count: usize, reverse: bool) -> Vec<VideoPacket> {
    let shards = (0..fragment_count)
        .map(|index| vec![index as u8; 64])
        .collect::<Vec<_>>();
    let mut packets = shards
        .iter()
        .enumerate()
        .filter(|(index, _)| {
            parity_count == 0
                || fec_group_for_fragment(fragment_count, *index as u16)
                    .is_none_or(|group| usize::from(group.start) != *index)
        })
        .map(|(index, payload)| data_packet(fragment_count, index as u16, payload))
        .collect::<Vec<_>>();
    if reverse {
        packets.reverse();
    }
    if parity_count > 0 {
        for group in fec_group_ranges(fragment_count) {
            let data = group
                .clone()
                .map(|index| shards[usize::from(index)].as_slice())
                .collect::<Vec<_>>();
            let parity = make_fec_parity_count(group.start, &data, parity_count)
                .expect("valid benchmark parity");
            for parity in parity {
                let length = parity.last_data_len.to_be_bytes();
                let mut payload = vec![parity.parity_index, length[0], length[1]];
                payload.extend_from_slice(&parity.bytes);
                packets.push(VideoPacket {
                    flags: VideoPacketFlags::FEC_PARITY,
                    stream_epoch: 1,
                    frame_id: 1,
                    pts_us: 1,
                    encoded_at_us: 1,
                    fragment_index: parity.group_start,
                    fragment_count,
                    payload: Bytes::from(payload),
                });
            }
        }
    }
    packets
}

fn data_packet(fragment_count: u16, fragment_index: u16, payload: &[u8]) -> VideoPacket {
    VideoPacket {
        flags: VideoPacketFlags::empty(),
        stream_epoch: 1,
        frame_id: 1,
        pts_us: 1,
        encoded_at_us: 1,
        fragment_index,
        fragment_count,
        payload: Bytes::copy_from_slice(payload),
    }
}
