use super::*;

use crate::shared_ring::layout::{RingMeta, SlotMeta};

#[test]
fn producer_consumer_roundtrip_in_two_handles() {
    let name = test_ring_name();
    let max = nv12_byte_size(PLACEHOLDER_WIDTH, PLACEHOLDER_HEIGHT);
    let mut producer = SharedFrameRingProducer::create(&name, max).expect("create");
    let consumer = SharedFrameRingConsumer::open(&name, max).expect("open");

    let frame = nv12_black(PLACEHOLDER_WIDTH, PLACEHOLDER_HEIGHT);
    let seq = producer
        .publish_nv12(
            PLACEHOLDER_WIDTH,
            PLACEHOLDER_HEIGHT,
            PLACEHOLDER_WIDTH,
            0,
            1,
            &frame,
        )
        .expect("publish");

    let view = consumer.latest_frame().expect("latest");
    assert_eq!(view.sequence, seq);
    assert_eq!(view.nv12.len(), frame.len());
    cleanup(&name);
}

#[test]
fn open_or_create_allows_consumer_attach() {
    let name = test_ring_name();
    let max = nv12_byte_size(64, 64);
    let mut producer = SharedFrameRingProducer::open_or_create(&name, max).expect("create");
    let consumer = SharedFrameRingConsumer::open(&name, max).expect("consumer");
    let frame = nv12_black(64, 64);
    producer
        .publish_nv12(64, 64, 64, 0, 9, &frame)
        .expect("publish");
    assert_eq!(consumer.latest_frame().expect("view").timestamp_us, 9);
    cleanup(&name);
}

#[test]
fn consumer_detects_replaced_named_mapping_generation() {
    let name = test_ring_name();
    let max = nv12_byte_size(64, 64);
    let first_producer = SharedFrameRingProducer::create(&name, max).expect("first producer");
    let consumer = SharedFrameRingConsumer::open(&name, max).expect("consumer");
    assert!(consumer.is_current_generation());

    drop(first_producer);
    assert!(!consumer.is_current_generation());
    let second_producer = SharedFrameRingProducer::create(&name, max).expect("second producer");
    assert!(!consumer.is_current_generation());
    let reattached = SharedFrameRingConsumer::open(&name, max).expect("reattached consumer");
    assert!(reattached.is_current_generation());

    drop((reattached, consumer, second_producer));
    cleanup(&name);
}

#[test]
fn ring_layout_is_stable() {
    assert_eq!(std::mem::size_of::<RingMeta>(), RING_META_SIZE);
    assert_eq!(std::mem::size_of::<SlotMeta>(), RING_SLOT_META_SIZE);
    assert_eq!(std::mem::offset_of!(RingMeta, latest_sequence), 24);
    assert_eq!(std::mem::offset_of!(SlotMeta, ready_state), 40);
    assert_eq!(std::mem::offset_of!(SlotMeta, reader_count), 44);
}

#[test]
fn rapid_overwrite_consumer_sees_latest_sequence() {
    let name = test_ring_name();
    let max = nv12_byte_size(64, 64);
    let mut producer = SharedFrameRingProducer::create(&name, max).expect("create");
    let consumer = SharedFrameRingConsumer::open(&name, max).expect("open");
    let frame = nv12_black(64, 64);

    let mut last_seq = 0u64;
    for i in 0..32u64 {
        last_seq = producer
            .publish_nv12(64, 64, 64, 0, i * 1_000, &frame)
            .expect("publish");
        // Interleave polls while overwriting (REQ-PICOO-FRAME-002).
        let _ = consumer.latest_frame();
    }
    let view = consumer.latest_frame().expect("latest");
    assert_eq!(view.sequence, last_seq);
    assert_eq!(view.timestamp_us, 31_000);
    cleanup(&name);
}

#[test]
fn leased_slots_are_never_overwritten() {
    let name = test_ring_name();
    let max = nv12_byte_size(64, 64);
    let mut producer = SharedFrameRingProducer::create(&name, max).expect("create");
    let consumer = SharedFrameRingConsumer::open(&name, max).expect("open");
    let frame = nv12_black(64, 64);

    producer
        .publish_nv12(64, 64, 64, 0, 1, &frame)
        .expect("first");
    let first = consumer.latest_frame().expect("lease first");
    producer
        .publish_nv12(64, 64, 64, 0, 2, &frame)
        .expect("second");
    let second = consumer.latest_frame().expect("lease second");
    producer
        .publish_nv12(64, 64, 64, 0, 3, &frame)
        .expect("third");
    let third = consumer.latest_frame().expect("lease third");

    let unchanged = producer
        .publish_nv12(64, 64, 64, 0, 4, &frame)
        .expect("drop while all slots leased");
    assert_eq!(unchanged, third.sequence);
    assert_eq!(first.timestamp_us, 1);
    assert_eq!(second.timestamp_us, 2);
    assert_eq!(third.timestamp_us, 3);
    drop((first, second, third));
    cleanup(&name);
}
