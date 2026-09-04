use super::*;
use crate::shared_ring::layout::{meta_at, slot_meta_at, WRITER_LEASE};
use crate::shared_ring::mapping::{ProducerMapping, SlotLockAttempt};
use std::sync::atomic::Ordering;

fn file_ring_path() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("{}.bin", test_ring_name()))
}

fn cleanup_file_ring(path: &std::path::Path) {
    for index in 0..crate::RING_SLOT_COUNT {
        let _ = std::fs::remove_file(crate::shared_ring::lock::slot_lock_path(path, index));
    }
    let _ = std::fs::remove_file(crate::shared_ring::lock::producer_lock_path(path));
    let _ = std::fs::remove_file(path);
}

#[test]
fn windows_machine_file_ring_roundtrips_and_reopens() {
    let path = file_ring_path();
    let max = nv12_byte_size(64, 64);
    let frame = nv12_black(64, 64);
    let mut producer =
        SharedFrameRingProducer::open_or_create_file(&path, max).expect("file producer");
    producer
        .publish_nv12(64, 64, 64, 0, 42, &frame)
        .expect("file publish");
    let consumer = SharedFrameRingConsumer::open_file(&path, max).expect("file consumer");
    let view = consumer.latest_frame().expect("file frame");
    assert_eq!(view.timestamp_us, 42);
    assert_eq!(view.nv12, frame);
    drop(view);
    drop((consumer, producer));

    let mut reopened =
        SharedFrameRingProducer::open_or_create_file(&path, max).expect("reopened producer");
    reopened
        .publish_nv12(64, 64, 64, 0, 43, &frame)
        .expect("reopened publish");
    let consumer = SharedFrameRingConsumer::open_file(&path, max).expect("reopened consumer");
    assert_eq!(
        consumer
            .latest_frame()
            .expect("reopened frame")
            .timestamp_us,
        43
    );

    drop((consumer, reopened));
    cleanup_file_ring(&path);
}

#[test]
fn windows_machine_file_ring_rejects_a_second_producer() {
    let path = file_ring_path();
    let max = nv12_byte_size(64, 64);
    let producer = SharedFrameRingProducer::open_or_create_file(&path, max).expect("file producer");
    let error = match SharedFrameRingProducer::open_or_create_file(&path, max) {
        Ok(_) => panic!("second file producer must fail"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        SharedRingError::ProducerAlreadyRunning(ref locked_path) if locked_path == &path
    ));

    drop(producer);
    cleanup_file_ring(&path);
}

#[test]
fn windows_machine_file_ring_reports_producer_lifecycle() {
    let path = file_ring_path();
    let max = nv12_byte_size(64, 64);
    let producer = SharedFrameRingProducer::open_or_create_file(&path, max).expect("file producer");
    let consumer = SharedFrameRingConsumer::open_file(&path, max).expect("file consumer");

    assert!(consumer.has_live_producer());
    drop(producer);
    assert!(!consumer.has_live_producer());

    drop(consumer);
    cleanup_file_ring(&path);
}

#[test]
fn windows_machine_file_ring_replaces_an_invalid_generation() {
    let path = file_ring_path();
    let max = nv12_byte_size(64, 64);
    let invalid =
        SharedFrameRingProducer::open_or_create_file(&path, max).expect("invalid file producer");
    let old_consumer = SharedFrameRingConsumer::open_file(&path, max).expect("old consumer");
    unsafe {
        (&mut *meta_at(invalid.mapping.as_ptr())).magic = 0;
    }
    drop(invalid);

    let mut replacement =
        SharedFrameRingProducer::open_or_create_file(&path, max).expect("replacement producer");
    assert!(!old_consumer.is_current_generation());
    let frame = nv12_black(64, 64);
    replacement
        .publish_nv12(64, 64, 64, 0, 44, &frame)
        .expect("replacement publish");
    let new_consumer = SharedFrameRingConsumer::open_file(&path, max).expect("new consumer");
    assert_eq!(
        new_consumer
            .latest_frame()
            .expect("replacement frame")
            .timestamp_us,
        44
    );

    drop((new_consumer, old_consumer, replacement));
    cleanup_file_ring(&path);
}

#[test]
#[ignore = "helper process for windows_machine_file_ring_recovers_after_process_exit"]
fn windows_crash_file_producer_helper() {
    let Some(path) = std::env::var_os("PICOO_TEST_CRASH_FILE_RING_PATH") else {
        return;
    };
    let max = nv12_byte_size(64, 64);
    let frame = nv12_black(64, 64);
    let mut producer =
        SharedFrameRingProducer::open_or_create_file(path, max).expect("crash file producer");
    producer
        .publish_nv12(64, 64, 64, 0, 1, &frame)
        .expect("crash file frame");
    std::process::exit(0);
}

#[test]
fn windows_machine_file_ring_recovers_after_process_exit() {
    let path = file_ring_path();
    let status = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--ignored",
            "--exact",
            "shared_ring::tests::windows::windows_crash_file_producer_helper",
            "--nocapture",
        ])
        .env("PICOO_TEST_CRASH_FILE_RING_PATH", &path)
        .status()
        .expect("crash file helper");
    assert!(status.success());

    let max = nv12_byte_size(64, 64);
    let frame = nv12_black(64, 64);
    let mut recovered =
        SharedFrameRingProducer::open_or_create_file(&path, max).expect("recovered file producer");
    recovered
        .publish_nv12(64, 64, 64, 0, 2, &frame)
        .expect("post-crash file frame");
    let consumer = SharedFrameRingConsumer::open_file(&path, max).expect("file consumer");
    assert_eq!(
        consumer
            .latest_frame()
            .expect("post-crash file frame")
            .timestamp_us,
        2
    );

    drop((consumer, recovered));
    cleanup_file_ring(&path);
}

#[test]
fn windows_kernel_lock_recovers_reader_lease_after_consumer_termination() {
    let name = test_ring_name();
    let max = nv12_byte_size(64, 64);
    let frame = nv12_black(64, 64);
    let mut producer = SharedFrameRingProducer::create(&name, max).expect("producer");
    producer
        .publish_nv12(64, 64, 64, 0, 1, &frame)
        .expect("first publish");

    let consumer = SharedFrameRingConsumer::open(&name, max).expect("consumer");
    let mut leaked = consumer.latest_frame().expect("leased frame");
    drop(leaked.kernel_lock.take());
    std::mem::forget(leaked);
    drop(consumer);
    // Model process termination: Windows closes the independent range-lock
    // handle, while the shared atomic count retains the abandoned lease.
    unsafe {
        (&*meta_at(producer.mapping.as_ptr()))
            .write_index
            .store(0, Ordering::SeqCst);
    }

    producer
        .publish_nv12(64, 64, 64, 0, 2, &frame)
        .expect("publish after terminated reader");
    let recovered = SharedFrameRingConsumer::open(&name, max).expect("new consumer");
    assert_eq!(recovered.latest_frame().expect("latest").timestamp_us, 2);

    drop(recovered);
    drop(producer);
    cleanup(&name);
}

#[test]
fn windows_lifecycle_lock_rejects_a_second_producer() {
    let name = test_ring_name();
    let max = nv12_byte_size(64, 64);
    let producer = SharedFrameRingProducer::create(&name, max).expect("producer");
    let error = match SharedFrameRingProducer::open_or_create(&name, max) {
        Ok(_) => panic!("second producer must fail"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        SharedRingError::ProducerAlreadyRunning(path)
            if path == SharedFrameRingProducer::flink_path(&name)
    ));

    drop(producer);
    cleanup(&name);
}

#[test]
fn windows_open_or_create_repairs_a_broken_generation_locator() {
    let name = test_ring_name();
    let max = nv12_byte_size(64, 64);
    let flink = SharedFrameRingProducer::flink_path(&name);
    std::fs::write(&flink, "/picoo-missing-mapping").expect("broken flink fixture");

    let mut producer =
        SharedFrameRingProducer::open_or_create(&name, max).expect("recovered producer");
    let frame = nv12_black(64, 64);
    producer
        .publish_nv12(64, 64, 64, 0, 7, &frame)
        .expect("publish");
    let consumer = SharedFrameRingConsumer::open(&name, max).expect("consumer");
    assert_eq!(consumer.latest_frame().expect("frame").timestamp_us, 7);

    drop((consumer, producer));
    cleanup(&name);
}

#[test]
fn windows_open_or_create_replaces_an_invalid_persisted_generation() {
    let name = test_ring_name();
    let max = nv12_byte_size(64, 64);
    let mut invalid = SharedFrameRingProducer::create(&name, max).expect("invalid producer");
    unsafe {
        (&mut *meta_at(invalid.mapping.as_ptr())).magic = 0;
    }
    let ProducerMapping::Shared(mapping) = &mut invalid.mapping else {
        panic!("named test ring must use shared mapping");
    };
    mapping.mapping.set_owner(false);
    drop(invalid);

    let mut recovered =
        SharedFrameRingProducer::open_or_create(&name, max).expect("replacement producer");
    let frame = nv12_black(64, 64);
    recovered
        .publish_nv12(64, 64, 64, 0, 8, &frame)
        .expect("publish");
    let consumer = SharedFrameRingConsumer::open(&name, max).expect("consumer");
    assert!(consumer.is_current_generation());
    assert_eq!(consumer.latest_frame().expect("frame").timestamp_us, 8);

    drop(consumer);
    drop(recovered);
    assert!(!SharedFrameRingProducer::flink_path(&name).exists());
    cleanup(&name);
}

#[test]
#[ignore = "helper process for windows_recovers_after_producer_process_termination"]
fn windows_crash_producer_helper() {
    let Ok(name) = std::env::var("PICOO_TEST_CRASH_RING_NAME") else {
        return;
    };
    let max = nv12_byte_size(64, 64);
    let frame = nv12_black(64, 64);
    let mut producer = SharedFrameRingProducer::create(&name, max).expect("crash producer");
    producer
        .publish_nv12(64, 64, 64, 0, 1, &frame)
        .expect("crash frame");
    std::process::exit(0);
}

#[test]
fn windows_recovers_after_producer_process_termination() {
    let name = test_ring_name();
    let status = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--ignored",
            "--exact",
            "shared_ring::tests::windows::windows_crash_producer_helper",
            "--nocapture",
        ])
        .env("PICOO_TEST_CRASH_RING_NAME", &name)
        .status()
        .expect("crash helper");
    assert!(status.success());

    let max = nv12_byte_size(64, 64);
    let frame = nv12_black(64, 64);
    let mut recovered =
        SharedFrameRingProducer::open_or_create(&name, max).expect("recovered producer");
    let consumer = SharedFrameRingConsumer::open(&name, max).expect("consumer");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    loop {
        recovered
            .publish_nv12(64, 64, 64, 0, 2, &frame)
            .expect("post-crash frame");
        if consumer
            .latest_frame()
            .is_some_and(|view| view.timestamp_us == 2)
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "post-crash slot locks were not released within one second"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    drop(consumer);
    drop(recovered);
    assert!(
        !SharedFrameRingProducer::flink_path(&name).exists(),
        "recovered Producer must adopt generation cleanup ownership"
    );
    cleanup(&name);
}

#[test]
fn windows_kernel_lock_recovers_writer_lease_after_producer_termination() {
    let name = test_ring_name();
    let max = nv12_byte_size(64, 64);
    let frame = nv12_black(64, 64);
    let mut producer = SharedFrameRingProducer::create(&name, max).expect("producer");
    producer
        .publish_nv12(64, 64, 64, 0, 1, &frame)
        .expect("publish");
    let writer_lock = match producer.mapping.try_slot_lock(0).expect("writer lock") {
        SlotLockAttempt::Acquired(lock) => lock,
        SlotLockAttempt::Busy => panic!("slot must lock"),
    };
    unsafe {
        (&*slot_meta_at(producer.mapping.as_ptr(), max, 0))
            .reader_count
            .store(WRITER_LEASE, Ordering::SeqCst);
    }
    drop(writer_lock);

    let consumer = SharedFrameRingConsumer::open(&name, max).expect("consumer");
    assert_eq!(
        consumer
            .latest_frame()
            .expect("recovered frame")
            .timestamp_us,
        1
    );

    drop(consumer);
    drop(producer);
    cleanup(&name);
}

#[test]
fn windows_shared_slot_locks_remain_held_until_every_view_is_dropped() {
    let name = test_ring_name();
    let max = nv12_byte_size(64, 64);
    let frame = nv12_black(64, 64);
    let mut producer = SharedFrameRingProducer::create(&name, max).expect("producer");
    producer
        .publish_nv12(64, 64, 64, 0, 1, &frame)
        .expect("publish");
    let consumer = SharedFrameRingConsumer::open(&name, max).expect("consumer");
    let first = consumer.latest_frame().expect("first reader");
    let second = consumer.latest_frame().expect("second reader");

    assert!(matches!(
        producer.mapping.try_slot_lock(0).expect("writer attempt"),
        SlotLockAttempt::Busy
    ));
    drop(first);
    assert!(matches!(
        producer
            .mapping
            .try_slot_lock(0)
            .expect("writer attempt after one reader"),
        SlotLockAttempt::Busy
    ));
    drop(second);
    assert!(matches!(
        producer
            .mapping
            .try_slot_lock(0)
            .expect("writer attempt after all readers"),
        SlotLockAttempt::Acquired(_)
    ));

    drop(consumer);
    drop(producer);
    cleanup(&name);
}

#[test]
fn windows_reader_falls_back_when_latest_slot_is_write_locked() {
    let name = test_ring_name();
    let max = nv12_byte_size(64, 64);
    let frame = nv12_black(64, 64);
    let mut producer = SharedFrameRingProducer::create(&name, max).expect("producer");
    producer
        .publish_nv12(64, 64, 64, 0, 1, &frame)
        .expect("first");
    producer
        .publish_nv12(64, 64, 64, 0, 2, &frame)
        .expect("second");
    let latest_lock = match producer.mapping.try_slot_lock(1).expect("latest lock") {
        SlotLockAttempt::Acquired(lock) => lock,
        SlotLockAttempt::Busy => panic!("latest slot must lock"),
    };

    let consumer = SharedFrameRingConsumer::open(&name, max).expect("consumer");
    let fallback = consumer.latest_frame().expect("fallback frame");
    assert_eq!(fallback.sequence, 1);
    assert_eq!(fallback.timestamp_us, 1);

    drop(fallback);
    drop(latest_lock);
    drop(consumer);
    drop(producer);
    cleanup(&name);
}
