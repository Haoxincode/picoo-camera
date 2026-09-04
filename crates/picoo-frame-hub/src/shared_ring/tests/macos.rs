use super::*;
use crate::shared_ring::file_mapping::open_file_mapping;
use crate::shared_ring::layout::{
    const_slot_meta_at, layout_size, meta_at, slot_meta_at, WRITER_LEASE,
};
use crate::shared_ring::lock::{producer_lock_path, slot_lock_path};
use crate::shared_ring::mapping::SlotLockAttempt;
use crate::RING_SLOT_COUNT;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

fn cleanup_file_ring(path: &Path) {
    let _ = std::fs::remove_file(path);
    for index in 0..RING_SLOT_COUNT {
        let _ = std::fs::remove_file(slot_lock_path(path, index));
    }
    let _ = std::fs::remove_file(producer_lock_path(path));
}

fn macos_reader_harness() -> PathBuf {
    let path = std::env::var_os("PICOO_MACOS_RING_READER_HARNESS")
        .map(PathBuf::from)
        .expect("run this test through `cargo xtask test macos`");
    assert!(
        path.is_file(),
        "missing Swift/C reader harness: {}",
        path.display()
    );
    path
}

fn patterned_nv12(timestamp_us: u64, max_frame_bytes: usize) -> Vec<u8> {
    (0..max_frame_bytes)
        .map(|offset| {
            ((timestamp_us + offset as u64 * 31 + (offset / 256) as u64 * 17) % 251) as u8
        })
        .collect()
}

const MACOS_CROSS_PROCESS_WIDTH: u32 = 64;
const MACOS_CROSS_PROCESS_HEIGHT: u32 = 64;
const MACOS_CROSS_PROCESS_STRIDE: u32 = 80;

fn macos_cross_process_frame_bytes() -> usize {
    (MACOS_CROSS_PROCESS_STRIDE * MACOS_CROSS_PROCESS_HEIGHT * 3 / 2) as usize
}

fn assert_process_success(output: std::process::Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn macos_extension_sources_match_shared_ring_identity_and_abi() {
    let c_source =
        include_str!("../../../../../extensions/macos-camera-extension/SharedRingAtomic.c");
    let swift_source =
        include_str!("../../../../../extensions/macos-camera-extension/SharedRingReader.swift");
    let entitlements = include_str!(
        "../../../../../extensions/macos-camera-extension/PicooCameraExtension.entitlements"
    );
    let info = include_str!("../../../../../extensions/macos-camera-extension/Info.plist");

    for expected in [
        format!("PICOO_RING_VERSION = {RING_VERSION}"),
        format!("PICOO_RING_META_SIZE = {RING_META_SIZE}"),
        format!("PICOO_RING_SLOT_COUNT = {RING_SLOT_COUNT}"),
        format!("PICOO_RING_SLOT_META_SIZE = {RING_SLOT_META_SIZE}"),
        format!("PICOO_RING_READY_DONE = {RING_READY_DONE}"),
    ] {
        assert!(c_source.contains(&expected), "C ABI drift: {expected}");
    }
    assert!(swift_source.contains(MACOS_APP_GROUP_INFO_KEY));
    assert!(entitlements.contains("@PICOO_APP_GROUP_IDENTIFIER@"));
    assert!(info.contains("group.com.haoxincode.picoo-camera"));
}

#[test]
fn file_backed_ring_roundtrip_matches_shared_layout() {
    let path = std::env::temp_dir().join(format!("{}.ring", test_ring_name()));
    let max = nv12_byte_size(64, 64);
    {
        let mut producer =
            SharedFrameRingProducer::open_or_create_file(&path, max).expect("file producer");
        let consumer = SharedFrameRingConsumer::open_file(&path, max).expect("file consumer");
        let frame = nv12_black(64, 64);
        producer
            .publish_nv12(64, 64, 64, 0, 42, &frame)
            .expect("publish");
        let view = consumer.latest_frame().expect("latest");
        assert_eq!(view.timestamp_us, 42);
        assert_eq!(view.nv12, frame);
    }
    cleanup_file_ring(&path);
}

#[test]
fn file_backed_ring_rejects_a_second_producer() {
    let path = std::env::temp_dir().join(format!("{}.ring", test_ring_name()));
    let max = nv12_byte_size(64, 64);
    let producer =
        SharedFrameRingProducer::open_or_create_file(&path, max).expect("first producer");

    let error = match SharedFrameRingProducer::open_or_create_file(&path, max) {
        Ok(_) => panic!("second producer must fail"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        SharedRingError::ProducerAlreadyRunning(ring_path) if ring_path == path
    ));

    drop(producer);
    cleanup_file_ring(&path);
}

#[test]
#[ignore = "requires the production Swift/C reader built by cargo xtask test macos"]
fn macos_rust_swift_cross_process_ring_contract() {
    use std::process::{Command, Stdio};

    let harness = macos_reader_harness();
    let max = macos_cross_process_frame_bytes();

    // Rust Writer and the production Swift/C Reader run concurrently in
    // separate processes. Every copied plane must match its timestamp,
    // proving the reader never observes partially overwritten NV12 data.
    let stress_path = std::env::temp_dir().join(format!("{}.ring", test_ring_name()));
    let ready_path = stress_path.with_extension("reader-ready");
    let mut producer =
        SharedFrameRingProducer::open_or_create_file(&stress_path, max).expect("producer");
    let mut reader = Command::new(&harness)
        .args([
            "stress",
            stress_path.to_str().expect("UTF-8 ring path"),
            ready_path.to_str().expect("UTF-8 ready path"),
            "192",
            "8",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn Swift/C stress reader");
    let ready_deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    while !ready_path.is_file() {
        if let Some(status) = reader.try_wait().expect("poll Swift/C stress reader") {
            panic!("Swift/C stress reader exited before readiness: {status}");
        }
        assert!(
            std::time::Instant::now() < ready_deadline,
            "Swift/C stress reader did not become ready"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    for timestamp_us in 1..=192 {
        producer
            .publish_nv12(
                MACOS_CROSS_PROCESS_WIDTH,
                MACOS_CROSS_PROCESS_HEIGHT,
                MACOS_CROSS_PROCESS_STRIDE,
                0,
                timestamp_us,
                &patterned_nv12(timestamp_us, max),
            )
            .expect("stress publish");
        std::thread::sleep(std::time::Duration::from_micros(750));
    }
    assert_process_success(
        reader
            .wait_with_output()
            .expect("wait for Swift/C stress reader"),
        "Swift/C stress reader",
    );
    drop(producer);
    let _ = std::fs::remove_file(&ready_path);
    cleanup_file_ring(&stress_path);

    // A killed Camera Extension leaves its shared atomic reader count
    // behind, while Darwin releases its flock descriptor. The Rust Writer
    // must reclaim that slot after wrapping the three-slot ring.
    let reader_crash_path = std::env::temp_dir().join(format!("{}.ring", test_ring_name()));
    let mut producer = SharedFrameRingProducer::open_or_create_file(&reader_crash_path, max)
        .expect("reader-crash producer");
    producer
        .publish_nv12(
            MACOS_CROSS_PROCESS_WIDTH,
            MACOS_CROSS_PROCESS_HEIGHT,
            MACOS_CROSS_PROCESS_STRIDE,
            0,
            1,
            &patterned_nv12(1, max),
        )
        .expect("seed reader-crash ring");
    let output = Command::new(&harness)
        .args([
            "leak-and-exit",
            reader_crash_path.to_str().expect("UTF-8 ring path"),
            "1",
        ])
        .output()
        .expect("spawn terminating Swift/C reader");
    assert_process_success(output, "terminating Swift/C reader");
    unsafe {
        assert_eq!(
            (&*slot_meta_at(producer.mapping.as_ptr(), max, 0))
                .reader_count
                .load(Ordering::SeqCst),
            1,
            "terminating Swift/C reader must leave its atomic lease behind"
        );
    }
    for timestamp_us in 2..=4 {
        producer
            .publish_nv12(
                MACOS_CROSS_PROCESS_WIDTH,
                MACOS_CROSS_PROCESS_HEIGHT,
                MACOS_CROSS_PROCESS_STRIDE,
                0,
                timestamp_us,
                &patterned_nv12(timestamp_us, max),
            )
            .expect("publish after reader termination");
    }
    unsafe {
        let recovered_slot = &*slot_meta_at(producer.mapping.as_ptr(), max, 0);
        assert_eq!(recovered_slot.reader_count.load(Ordering::SeqCst), 0);
        assert_eq!(recovered_slot.timestamp_us, 4);
    }
    let consumer =
        SharedFrameRingConsumer::open_file(&reader_crash_path, max).expect("Rust consumer");
    assert_eq!(
        consumer
            .latest_frame()
            .expect("recovered latest frame")
            .timestamp_us,
        4
    );
    drop((consumer, producer));
    cleanup_file_ring(&reader_crash_path);

    // A Rust Producer killed while holding both its lifecycle lock and a
    // slot writer lease must not strand the Swift/C Reader or replacement
    // Producer. The kernel releases both flock descriptors at exit.
    let producer_crash_path = std::env::temp_dir().join(format!("{}.ring", test_ring_name()));
    let status = Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--ignored",
            "--exact",
            "shared_ring::tests::macos::macos_crash_file_producer_helper",
            "--nocapture",
        ])
        .env("PICOO_TEST_CRASH_RING_PATH", &producer_crash_path)
        .status()
        .expect("spawn terminating Rust producer");
    assert!(
        status.success(),
        "terminating Rust producer failed: {status}"
    );
    let abandoned =
        open_file_mapping(&producer_crash_path, max).expect("inspect abandoned producer mapping");
    unsafe {
        assert_eq!(
            (&*const_slot_meta_at(abandoned.mapping.as_ptr(), max, 0))
                .reader_count
                .load(Ordering::SeqCst),
            WRITER_LEASE,
            "terminating Rust producer must leave its writer lease behind"
        );
    }
    drop(abandoned);
    let output = Command::new(&harness)
        .args([
            "read-once",
            producer_crash_path.to_str().expect("UTF-8 ring path"),
            "1",
        ])
        .output()
        .expect("spawn Swift/C recovery reader");
    assert_process_success(output, "Swift/C recovery reader");
    let recovered =
        open_file_mapping(&producer_crash_path, max).expect("inspect Swift/C recovered mapping");
    unsafe {
        assert_eq!(
            (&*const_slot_meta_at(recovered.mapping.as_ptr(), max, 0))
                .reader_count
                .load(Ordering::SeqCst),
            0,
            "Swift/C Reader must clear and release the abandoned writer lease"
        );
    }
    drop(recovered);

    let mut replacement = SharedFrameRingProducer::open_or_create_file(&producer_crash_path, max)
        .expect("replacement producer");
    replacement
        .publish_nv12(
            MACOS_CROSS_PROCESS_WIDTH,
            MACOS_CROSS_PROCESS_HEIGHT,
            MACOS_CROSS_PROCESS_STRIDE,
            0,
            2,
            &patterned_nv12(2, max),
        )
        .expect("replacement publish");
    let consumer = SharedFrameRingConsumer::open_file(&producer_crash_path, max)
        .expect("replacement consumer");
    assert_eq!(
        consumer
            .latest_frame()
            .expect("replacement frame")
            .timestamp_us,
        2
    );
    drop((consumer, replacement));
    cleanup_file_ring(&producer_crash_path);
}

#[test]
#[ignore = "helper process for macos_rust_swift_cross_process_ring_contract"]
fn macos_crash_file_producer_helper() {
    let Some(path) = std::env::var_os("PICOO_TEST_CRASH_RING_PATH").map(PathBuf::from) else {
        return;
    };
    let max = macos_cross_process_frame_bytes();
    let mut producer =
        SharedFrameRingProducer::open_or_create_file(&path, max).expect("crash producer");
    producer
        .publish_nv12(
            MACOS_CROSS_PROCESS_WIDTH,
            MACOS_CROSS_PROCESS_HEIGHT,
            MACOS_CROSS_PROCESS_STRIDE,
            0,
            1,
            &patterned_nv12(1, max),
        )
        .expect("crash producer seed frame");
    let _writer_lock = match producer.mapping.try_slot_lock(0).expect("writer lock") {
        SlotLockAttempt::Acquired(lock) => lock,
        SlotLockAttempt::Busy | SlotLockAttempt::NotFile => panic!("file slot must lock"),
    };
    unsafe {
        (&*slot_meta_at(producer.mapping.as_ptr(), max, 0))
            .reader_count
            .store(WRITER_LEASE, Ordering::SeqCst);
    }
    std::process::exit(0);
}

#[test]
fn file_backed_ring_recovers_across_producer_restart_and_stale_abi() {
    let path = std::env::temp_dir().join(format!("{}.ring", test_ring_name()));
    let max = nv12_byte_size(64, 64);
    let frame = nv12_black(64, 64);

    {
        let mut producer =
            SharedFrameRingProducer::open_or_create_file(&path, max).expect("first producer");
        producer
            .publish_nv12(64, 64, 64, 0, 1, &frame)
            .expect("first publish");
    }
    {
        let mut restarted =
            SharedFrameRingProducer::open_or_create_file(&path, max).expect("restart");
        let consumer = SharedFrameRingConsumer::open_file(&path, max).expect("consumer");
        assert_eq!(
            consumer
                .latest_frame()
                .expect("preserved frame")
                .timestamp_us,
            1
        );
        restarted
            .publish_nv12(64, 64, 64, 0, 2, &frame)
            .expect("publish after restart");
        assert_eq!(consumer.latest_frame().expect("new frame").timestamp_us, 2);
    }

    std::fs::write(&path, vec![0; layout_size(max)]).expect("stale ABI fixture");
    let mut recovered =
        SharedFrameRingProducer::open_or_create_file(&path, max).expect("replace stale ABI");
    recovered
        .publish_nv12(64, 64, 64, 0, 3, &frame)
        .expect("publish after ABI replacement");
    let consumer = SharedFrameRingConsumer::open_file(&path, max).expect("recovered consumer");
    assert_eq!(
        consumer
            .latest_frame()
            .expect("recovered frame")
            .timestamp_us,
        3
    );

    drop((consumer, recovered));
    cleanup_file_ring(&path);
}

#[test]
fn file_lock_recovers_reader_lease_after_consumer_termination() {
    let path = std::env::temp_dir().join(format!("{}.ring", test_ring_name()));
    let max = nv12_byte_size(64, 64);
    let frame = nv12_black(64, 64);
    let mut producer = SharedFrameRingProducer::open_or_create_file(&path, max).expect("producer");
    producer
        .publish_nv12(64, 64, 64, 0, 1, &frame)
        .expect("first publish");

    let consumer = SharedFrameRingConsumer::open_file(&path, max).expect("consumer");
    let mut leaked = consumer.latest_frame().expect("leased frame");
    drop(leaked.kernel_lock.take());
    std::mem::forget(leaked);
    drop(consumer);
    // The independent lease descriptor has been closed as the kernel
    // would do on process exit, while the shared atomic count remains.
    unsafe {
        (&*meta_at(producer.mapping.as_ptr()))
            .write_index
            .store(0, Ordering::SeqCst);
    }

    producer
        .publish_nv12(64, 64, 64, 0, 2, &frame)
        .expect("publish after terminated reader");
    let recovered = SharedFrameRingConsumer::open_file(&path, max).expect("new consumer");
    assert_eq!(recovered.latest_frame().expect("latest").timestamp_us, 2);

    drop((recovered, producer));
    cleanup_file_ring(&path);
}

#[test]
fn per_slot_file_locks_allow_parallel_write_and_multiple_views() {
    let path = std::env::temp_dir().join(format!("{}.ring", test_ring_name()));
    let max = nv12_byte_size(64, 64);
    let frame = nv12_black(64, 64);
    let mut producer = SharedFrameRingProducer::open_or_create_file(&path, max).expect("producer");
    let consumer = SharedFrameRingConsumer::open_file(&path, max).expect("consumer");

    producer
        .publish_nv12(64, 64, 64, 0, 1, &frame)
        .expect("first");
    let first_a = consumer.latest_frame().expect("first lease A");
    let first_b = consumer.latest_frame().expect("first lease B");
    producer
        .publish_nv12(64, 64, 64, 0, 2, &frame)
        .expect("second while slot zero is read");
    let second = consumer.latest_frame().expect("second lease");
    producer
        .publish_nv12(64, 64, 64, 0, 3, &frame)
        .expect("third while two slots are read");
    let third = consumer.latest_frame().expect("third lease");

    drop(first_a);
    let unchanged = producer
        .publish_nv12(64, 64, 64, 0, 4, &frame)
        .expect("all slots remain protected");
    assert_eq!(unchanged, third.sequence);

    drop(first_b);
    let fourth = producer
        .publish_nv12(64, 64, 64, 0, 4, &frame)
        .expect("slot zero becomes writable");
    assert_eq!(fourth, third.sequence + 1);
    assert_eq!(second.timestamp_us, 2);
    assert_eq!(third.timestamp_us, 3);

    drop(second);
    drop(third);
    drop(consumer);
    drop(producer);
    cleanup_file_ring(&path);
}

#[test]
fn reader_falls_back_when_latest_file_slot_is_write_locked() {
    let path = std::env::temp_dir().join(format!("{}.ring", test_ring_name()));
    let max = nv12_byte_size(64, 64);
    let frame = nv12_black(64, 64);
    let mut producer = SharedFrameRingProducer::open_or_create_file(&path, max).expect("producer");
    producer
        .publish_nv12(64, 64, 64, 0, 1, &frame)
        .expect("first");
    producer
        .publish_nv12(64, 64, 64, 0, 2, &frame)
        .expect("second");
    let latest_lock = match producer.mapping.try_slot_lock(1).expect("latest lock") {
        SlotLockAttempt::Acquired(lock) => lock,
        SlotLockAttempt::Busy | SlotLockAttempt::NotFile => panic!("file slot must lock"),
    };

    let consumer = SharedFrameRingConsumer::open_file(&path, max).expect("consumer");
    let fallback = consumer.latest_frame().expect("fallback frame");
    assert_eq!(fallback.sequence, 1);
    assert_eq!(fallback.timestamp_us, 1);

    drop(fallback);
    drop(latest_lock);
    drop(consumer);
    drop(producer);
    cleanup_file_ring(&path);
}

#[test]
fn file_lock_recovers_writer_lease_after_producer_termination() {
    let path = std::env::temp_dir().join(format!("{}.ring", test_ring_name()));
    let max = nv12_byte_size(64, 64);
    let frame = nv12_black(64, 64);
    {
        let mut producer =
            SharedFrameRingProducer::open_or_create_file(&path, max).expect("producer");
        producer
            .publish_nv12(64, 64, 64, 0, 1, &frame)
            .expect("publish");
        // Model a producer killed after acquiring its atomic lease. Its
        // file descriptor closes at scope exit, so the kernel lock cannot leak.
        let base = producer.mapping.as_ptr();
        unsafe {
            (&*slot_meta_at(base, max, 0))
                .reader_count
                .store(WRITER_LEASE, Ordering::SeqCst);
        }
    }

    let consumer = SharedFrameRingConsumer::open_file(&path, max).expect("consumer");
    assert_eq!(
        consumer
            .latest_frame()
            .expect("recovered frame")
            .timestamp_us,
        1
    );

    drop(consumer);
    cleanup_file_ring(&path);
}
