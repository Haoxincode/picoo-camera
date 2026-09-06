use super::*;
use crate::shared_ring::layout::{meta_at, slot_meta_at};

#[test]
fn invalidation_does_not_wait_for_or_overwrite_a_reader_lease() {
    let name = test_ring_name();
    let mut producer = SharedFrameRingProducer::create(&name, 6).unwrap();
    let fence = producer.content_fence();
    let first_generation = fence.current();
    let consumer = SharedFrameRingConsumer::open(&name, 6).unwrap();
    producer
        .publish_nv12_in_generation(first_generation, 2, 2, 2, 0, 1, &[7; 6])
        .unwrap();
    let retained = consumer.latest_frame().unwrap();
    let other_thread = fence.clone();
    let generation = std::thread::spawn(move || other_thread.invalidate())
        .join()
        .unwrap();
    assert_ne!(generation, first_generation);
    assert!(consumer.latest_frame().is_none());
    assert_eq!(
        retained.nv12, &[7; 6],
        "invalidation never writes leased pixels"
    );
    assert!(matches!(
        producer.publish_nv12_in_generation(first_generation, 2, 2, 2, 0, 2, &[9; 6]),
        Err(SharedRingError::ContentInvalidated)
    ));
    producer
        .publish_nv12_in_generation(generation, 2, 2, 2, 0, 3, &[11; 6])
        .unwrap();
    assert_eq!(consumer.latest_frame().unwrap().timestamp_us, 3);
    drop(retained);
    drop((consumer, producer, fence));
    cleanup(&name);
}

#[test]
fn late_old_slot_commit_cannot_reappear_after_invalidation() {
    let name = test_ring_name();
    let mut producer = SharedFrameRingProducer::create(&name, 6).unwrap();
    let consumer = SharedFrameRingConsumer::open(&name, 6).unwrap();
    let fence = producer.content_fence();
    let old = fence.current();
    producer.publish_nv12(2, 2, 2, 0, 1, &[7; 6]).unwrap();
    fence.invalidate();
    // Reproduce the final-commit race: an old prepared slot becomes ready
    // AFTER owner invalidation, even though the worker checked before copying.
    unsafe {
        let slot = &*slot_meta_at(producer.mapping.as_ptr(), 6, 0);
        slot.content_generation.store(old, Ordering::SeqCst);
        slot.sequence.store(100, Ordering::Release);
        slot.ready_state.store(RING_READY_DONE, Ordering::Release);
        (&*meta_at(producer.mapping.as_ptr()))
            .latest_sequence
            .store(100, Ordering::Release);
    }
    assert!(consumer.latest_frame().is_none());
    producer.publish_nv12(2, 2, 2, 0, 2, &[11; 6]).unwrap();
    assert_eq!(consumer.latest_frame().unwrap().timestamp_us, 2);
    drop((consumer, producer, fence));
    cleanup(&name);
}

#[test]
fn exhausted_generation_remains_closed_and_handle_retains_its_mapping() {
    let name = test_ring_name();
    let mut producer = SharedFrameRingProducer::create(&name, 6).unwrap();
    let fence = producer.content_fence();
    unsafe {
        (&*meta_at(producer.mapping.as_ptr()))
            .content_generation
            .store(u64::MAX - 1, Ordering::SeqCst);
    }
    assert_eq!(fence.invalidate(), u64::MAX);
    assert_eq!(fence.invalidate(), 0);
    assert_eq!(fence.invalidate(), 0);
    assert!(matches!(
        producer.publish_nv12(2, 2, 2, 0, 1, &[7; 6]),
        Err(SharedRingError::ContentInvalidated)
    ));
    drop(producer);
    assert_eq!(fence.current(), 0);
    drop(fence);
    cleanup(&name);
}
