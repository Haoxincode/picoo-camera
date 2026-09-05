use super::*;

use crate::shared_ring::layout::{RingMeta, SlotMeta};

#[test]
fn producer_consumer_roundtrip_in_two_handles() {
    let name = test_ring_name();
    let max = nv12_byte_size(PLACEHOLDER_WIDTH, PLACEHOLDER_HEIGHT);
    let mut producer = SharedFrameRingProducer::create(&name, max).expect("create");
    let consumer = SharedFrameRingConsumer::open(&name, max).expect("open");

    let frame = nv12_black(PLACEHOLDER_WIDTH, PLACEHOLDER_HEIGHT);
    let outcome = producer
        .publish_nv12(
            PLACEHOLDER_WIDTH,
            PLACEHOLDER_HEIGHT,
            PLACEHOLDER_WIDTH,
            0,
            1,
            &frame,
        )
        .expect("publish");
    let RingPublishOutcome::Published { sequence: seq } = outcome else {
        panic!("initial publish unexpectedly busy");
    };

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
fn miri_raw_layout_views_stay_aligned_and_within_mapping() {
    use std::alloc::{alloc_zeroed, dealloc, handle_alloc_error, Layout};
    use std::ptr::NonNull;
    use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

    use crate::shared_ring::layout::{
        const_slot_meta_at, const_slot_pixels_at, layout_size, meta_at, slot_meta_at,
        slot_pixels_at, validate_ring_header,
    };

    struct AlignedMapping {
        base: NonNull<u8>,
        layout: Layout,
    }

    impl Drop for AlignedMapping {
        fn drop(&mut self) {
            // SAFETY: `base` was allocated with this exact layout and remains owned here.
            unsafe { dealloc(self.base.as_ptr(), self.layout) };
        }
    }

    // An odd capacity exercises the stride padding required before every
    // SlotMeta. Production's default capacity happens to be naturally aligned.
    let max_frame_bytes = 127;
    let layout = Layout::from_size_align(
        layout_size(max_frame_bytes),
        std::mem::align_of::<RingMeta>(),
    )
    .expect("valid mapping layout");
    // SAFETY: The non-zero layout is retained by AlignedMapping for deallocation.
    let base =
        NonNull::new(unsafe { alloc_zeroed(layout) }).unwrap_or_else(|| handle_alloc_error(layout));
    let mapping = AlignedMapping { base, layout };

    // SAFETY: The allocation covers the complete computed layout, is aligned for
    // RingMeta/SlotMeta, and each object is initialized before it is referenced.
    unsafe {
        meta_at(mapping.base.as_ptr()).write(RingMeta {
            magic: RING_MAGIC,
            version: RING_VERSION,
            slot_count: RING_SLOT_COUNT as u32,
            max_frame_bytes: max_frame_bytes as u32,
            write_index: AtomicU32::new(0),
            latest_sequence: AtomicU64::new(0),
            _pad: [0; 32],
        });
        for index in 0..RING_SLOT_COUNT {
            slot_meta_at(mapping.base.as_ptr(), max_frame_bytes, index).write(SlotMeta {
                sequence: AtomicU64::new(index as u64 + 1),
                timestamp_us: 0,
                width: 0,
                height: 0,
                stride: 0,
                rotation: 0,
                pixel_format: PIXEL_FORMAT_NV12,
                data_length: max_frame_bytes as u32,
                ready_state: AtomicU32::new(RING_READY_DONE),
                reader_count: AtomicU32::new(0),
                _pad: [0; 16],
            });
            let pixels = slot_pixels_at(mapping.base.as_ptr(), max_frame_bytes, index);
            pixels.fill(index as u8 + 1);
        }

        validate_ring_header(mapping.base.as_ptr(), max_frame_bytes).expect("valid header");
        for index in 0..RING_SLOT_COUNT {
            let meta = &*const_slot_meta_at(mapping.base.as_ptr(), max_frame_bytes, index);
            assert_eq!(meta.sequence.load(Ordering::Acquire), index as u64 + 1);
            let pixels = const_slot_pixels_at(mapping.base.as_ptr(), max_frame_bytes, index);
            assert_eq!(pixels, vec![index as u8 + 1; max_frame_bytes]);
        }
    }
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
        let outcome = producer
            .publish_nv12(64, 64, 64, 0, i * 1_000, &frame)
            .expect("publish");
        let RingPublishOutcome::Published { sequence } = outcome else {
            panic!("unleased publish unexpectedly busy");
        };
        last_seq = sequence;
        // Interleave polls while overwriting (REQ-PICOO-FRAME-002).
        let _ = consumer.latest_frame();
    }
    let view = consumer.latest_frame().expect("latest");
    assert_eq!(view.sequence, last_seq);
    assert_eq!(view.timestamp_us, 31_000);
    cleanup(&name);
}

#[test]
fn smaller_frame_exposes_only_its_declared_data_length() {
    let name = test_ring_name();
    let max = nv12_byte_size(128, 128);
    let mut producer = SharedFrameRingProducer::create(&name, max).expect("create");
    let consumer = SharedFrameRingConsumer::open(&name, max).expect("open");
    let large = vec![0xA5; max];
    producer
        .publish_nv12(128, 128, 128, 0, 1, &large)
        .expect("large publish");
    let small = vec![0x3C; nv12_byte_size(64, 64)];
    producer
        .publish_nv12(64, 64, 64, 0, 2, &small)
        .expect("small publish");

    let view = consumer.latest_frame().expect("latest");
    assert_eq!(view.nv12, small.as_slice());
    assert_eq!(view.nv12.len(), nv12_byte_size(64, 64));
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

    let outcome = producer
        .publish_nv12(64, 64, 64, 0, 4, &frame)
        .expect("drop while all slots leased");
    assert_eq!(outcome, RingPublishOutcome::Busy);
    assert_eq!(first.timestamp_us, 1);
    assert_eq!(second.timestamp_us, 2);
    assert_eq!(third.timestamp_us, 3);
    drop((first, second, third));
    cleanup(&name);
}
