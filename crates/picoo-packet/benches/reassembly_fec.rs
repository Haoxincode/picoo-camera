use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;
use picoo_packet::ReassemblyMap;
use picoo_protocol::{
    fec_group_for_fragment, fec_group_ranges, make_fec_parity_count, VideoPacket, VideoPacketFlags,
    MAX_FEC_FRAGMENT_PAYLOAD,
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

#[derive(Clone, Copy)]
enum MapMode {
    Cold,
    Reused,
}

impl MapMode {
    const fn label(self) -> &'static str {
        match self {
            Self::Cold => "cold",
            Self::Reused => "reused",
        }
    }
}

fn main() {
    for payload_size in [64, MAX_FEC_FRAGMENT_PAYLOAD] {
        for fragment_count in [12, 64, 256] {
            for mode in [MapMode::Cold, MapMode::Reused] {
                run(fragment_count, payload_size, 0, false, "clean", mode);
                run(fragment_count, payload_size, 0, true, "reverse", mode);
                run(fragment_count, payload_size, 1, true, "light-loss", mode);
                run(fragment_count, payload_size, 2, true, "strong-loss", mode);
            }
        }
    }
}

fn run(
    fragment_count: u16,
    payload_size: usize,
    parity_count: usize,
    reverse: bool,
    label: &str,
    mode: MapMode,
) {
    let template = packets(fragment_count, payload_size, parity_count, reverse);
    let mut reused = ReassemblyMap::new(8, 1_024);
    ALLOCATIONS.store(0, Ordering::Relaxed);
    let started = Instant::now();
    let mut submitted = 0_u64;
    let mut completed = 0_u64;
    let mut checks = 0_u64;
    let mut attempts = 0_u64;
    while started.elapsed() < SAMPLE_TIME {
        submitted += 1;
        let (frame_completed, frame_checks, frame_attempts) = match mode {
            MapMode::Cold => {
                let mut cold = ReassemblyMap::new(8, 1_024);
                ingest_frame(&mut cold, &template, submitted)
            }
            MapMode::Reused => ingest_frame(&mut reused, &template, submitted),
        };
        completed += frame_completed;
        checks += frame_checks;
        attempts += frame_attempts;
    }
    assert_eq!(completed, submitted, "every recoverable AU must complete");
    let elapsed = started.elapsed().as_secs_f64();
    let allocations = ALLOCATIONS.load(Ordering::Relaxed);
    println!(
        "{fragment_count:>3} x {payload_size:>4}B {label:<11} {:<6}: {:>8.1} completed AU/s, {:>7.1} allocations/AU, {:>6.1} group checks/AU, {:>5.1} recoveries/AU",
        mode.label(),
        completed as f64 / elapsed,
        allocations as f64 / completed as f64,
        checks as f64 / completed as f64,
        attempts as f64 / completed as f64,
    );
}

fn ingest_frame(
    reassembly: &mut ReassemblyMap,
    template: &[VideoPacket],
    frame_id: u64,
) -> (u64, u64, u64) {
    let checks_before = reassembly.fec_group_check_count();
    let attempts_before = reassembly.fec_recovery_attempt_count();
    let mut completed = 0;
    for mut packet in template.iter().cloned() {
        packet.frame_id = frame_id;
        packet.pts_us = frame_id;
        completed +=
            u64::from(black_box(reassembly.ingest(packet).expect("benchmark packet")).is_some());
    }
    (
        completed,
        reassembly
            .fec_group_check_count()
            .saturating_sub(checks_before),
        reassembly
            .fec_recovery_attempt_count()
            .saturating_sub(attempts_before),
    )
}

fn packets(
    fragment_count: u16,
    payload_size: usize,
    parity_count: usize,
    reverse: bool,
) -> Vec<VideoPacket> {
    let shards = (0..fragment_count)
        .map(|index| vec![index as u8; payload_size])
        .collect::<Vec<_>>();
    let mut packets = shards
        .iter()
        .enumerate()
        .filter(|(index, _)| {
            parity_count == 0
                || fec_group_for_fragment(fragment_count, *index as u16).is_none_or(|group| {
                    let offset = (*index as u16).saturating_sub(group.start);
                    usize::from(offset) >= parity_count
                })
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
