//! Phone/QUIC-independent VCam contract harness — REQ-PICOO-VCAM-012.

use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

use picoo_frame_hub::{SharedFrameRingProducer, DEFAULT_MAX_FRAME_BYTES};

use crate::format::{nv12_len, SAMPLE_DURATION_100NS};
use crate::frame_provider::{FrameOrigin, FrameProvider};
use crate::sample_clock::SampleClock;

fn ring_name() -> String {
    format!(
        "vcam-contract-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    )
}

fn wait_for_pixels(provider: &FrameProvider, value: u8) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let acquired = provider
            .acquire_for_output(854, 480)
            .expect("negotiated output");
        if acquired.frame.pixels.first() == Some(&value) {
            assert_eq!(acquired.origin, FrameOrigin::Fresh);
            assert_eq!(
                (
                    acquired.frame.width,
                    acquired.frame.height,
                    acquired.frame.stride,
                    acquired.frame.pixels.len(),
                ),
                (854, 480, 854, nv12_len(854, 480).expect("NV12 size"))
            );
            return;
        }
        assert!(Instant::now() < deadline, "prepared frame did not arrive");
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn producer_pause_crash_restart_and_host_rebuild_preserve_contract() {
    let name = ring_name();
    let frame_len = nv12_len(854, 480).expect("NV12 size");
    let mut producer = SharedFrameRingProducer::create(&name, DEFAULT_MAX_FRAME_BYTES)
        .expect("first producer generation");
    producer
        .publish_nv12(854, 480, 854, 0, 1, &vec![41; frame_len])
        .expect("first frame");

    let first_host = FrameProvider::with_ring_name(name.clone()).expect("first host");
    let second_client = FrameProvider::with_ring_name(name.clone()).expect("second client");
    wait_for_pixels(&first_host, 41);
    wait_for_pixels(&second_client, 41);

    thread::sleep(Duration::from_millis(600));
    let cached = first_host
        .acquire_for_output(854, 480)
        .expect("cached paused frame");
    assert_eq!(cached.origin, FrameOrigin::Cached);
    assert_eq!(cached.frame.pixels.first(), Some(&41));

    drop(producer);
    let placeholder_deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if first_host
            .acquire_for_output(854, 480)
            .is_some_and(|frame| frame.origin == FrameOrigin::Placeholder)
        {
            break;
        }
        assert!(
            Instant::now() < placeholder_deadline,
            "producer crash did not reach placeholder"
        );
        thread::sleep(Duration::from_millis(10));
    }

    let mut replacement = SharedFrameRingProducer::create(&name, DEFAULT_MAX_FRAME_BYTES)
        .expect("replacement producer generation");
    replacement
        .publish_nv12(854, 480, 854, 0, 2, &vec![82; frame_len])
        .expect("replacement frame with sequence reset");
    wait_for_pixels(&first_host, 82);
    wait_for_pixels(&second_client, 82);

    first_host.shutdown();
    drop(first_host);
    let rebuilt_host = FrameProvider::with_ring_name(name.clone()).expect("rebuilt host");
    wait_for_pixels(&rebuilt_host, 82);

    rebuilt_host.shutdown();
    second_client.shutdown();
    drop((rebuilt_host, second_client, replacement));
    let _ = std::fs::remove_file(SharedFrameRingProducer::flink_path(&name));
}

#[test]
fn concurrent_requests_and_repeated_shutdown_are_safe() {
    let provider = Arc::new(FrameProvider::with_ring_name(ring_name()).expect("provider"));
    let start = Arc::new(Barrier::new(5));
    let readers = (0..4)
        .map(|_| {
            let provider = Arc::clone(&provider);
            let start = Arc::clone(&start);
            thread::spawn(move || {
                start.wait();
                for _ in 0..2_000 {
                    let frame = provider
                        .acquire_for_output(1920, 1080)
                        .expect("legal output during shutdown race");
                    assert_eq!(frame.frame.stride, 1920);
                    assert_eq!(
                        frame.frame.pixels.len(),
                        nv12_len(1920, 1080).expect("1080p NV12")
                    );
                    thread::yield_now();
                }
            })
        })
        .collect::<Vec<_>>();

    start.wait();
    provider.shutdown();
    provider.shutdown();
    for reader in readers {
        reader.join().expect("request worker");
    }
    assert!(provider.acquire_for_output(1280, 720).is_some());
}

#[test]
fn every_negotiated_shape_has_fixed_stride_size_and_monotonic_clock() {
    let provider = FrameProvider::with_ring_name(ring_name()).expect("provider");
    for (width, height) in [(854, 480), (1280, 720), (1920, 1080)] {
        let frame = provider
            .acquire_for_output(width, height)
            .expect("supported negotiated output")
            .frame;
        assert_eq!(
            (frame.width, frame.height, frame.stride),
            (width, height, width)
        );
        assert_eq!(frame.pixels.len(), nv12_len(width, height).expect("NV12"));
    }

    let mut clock = SampleClock::new(SAMPLE_DURATION_100NS);
    let mut previous = clock.next_timestamp(1_000_000).expect("first timestamp");
    for request in 1..1_000_i64 {
        let next = clock
            .next_timestamp(1_000_000 + request)
            .expect("timestamp");
        assert_eq!(next - previous, SAMPLE_DURATION_100NS);
        previous = next;
    }
    clock.reset();
    assert_eq!(clock.next_timestamp(9_000_000), Some(9_000_000));
    provider.shutdown();
}
