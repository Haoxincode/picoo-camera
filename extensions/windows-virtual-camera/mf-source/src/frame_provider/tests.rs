use super::preparation::fit_nv12;
use super::*;
use picoo_frame_hub::{
    waiting_placeholder_for_size, SharedFrameRingProducer, PLACEHOLDER_HEIGHT, PLACEHOLDER_WIDTH,
};

fn test_ring_name() -> String {
    format!(
        "frame-provider-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    )
}

#[test]
fn starts_with_shared_branded_placeholder() {
    let provider =
        FrameProvider::with_ring_name(test_ring_name()).expect("isolated frame provider");
    let acquired = provider
        .acquire_for_output(1280, 720)
        .expect("supported output");
    let frame = acquired.frame;
    assert_eq!(acquired.origin, FrameOrigin::Placeholder);
    assert_eq!((frame.width, frame.height), (1280, 720));
    assert_eq!(
        frame.pixels.as_ref(),
        waiting_placeholder_for_size(1280, 720).as_slice()
    );

    let negotiated = provider
        .acquire_for_output(1920, 1080)
        .expect("supported output");
    assert_eq!(negotiated.origin, FrameOrigin::Placeholder);
    assert_eq!(
        (negotiated.frame.width, negotiated.frame.height),
        (1920, 1080)
    );
    assert_eq!(
        negotiated.frame.pixels.as_ref(),
        waiting_placeholder_for_size(1920, 1080).as_slice()
    );
    let cached_pixels = negotiated.frame.pixels.as_ptr();
    let repeated = provider
        .acquire_for_output(1920, 1080)
        .expect("supported output");
    assert_eq!(repeated.origin, FrameOrigin::Placeholder);
    assert_eq!(
        repeated.frame.pixels.as_ptr(),
        cached_pixels,
        "cache hits must share immutable pixels instead of cloning the frame"
    );
    assert!(provider.acquire_for_output(640, 480).is_none());
}

#[test]
fn reconnects_to_new_mapping_generation_even_when_sequence_restarts() {
    let ring_name = test_ring_name();
    let frame_len = nv12_len(PLACEHOLDER_WIDTH, PLACEHOLDER_HEIGHT).expect("NV12 size");
    let first_pixels = vec![1; frame_len];
    let second_pixels = vec![2; frame_len];
    let mut first_producer = SharedFrameRingProducer::create(&ring_name, DEFAULT_MAX_FRAME_BYTES)
        .expect("first producer");
    first_producer
        .publish_nv12(
            PLACEHOLDER_WIDTH,
            PLACEHOLDER_HEIGHT,
            PLACEHOLDER_WIDTH,
            0,
            1,
            &first_pixels,
        )
        .expect("first frame");
    let mut provider = RingFrameReader::with_ring_name(ring_name.clone());
    let acquired = provider.acquire();
    assert_eq!(acquired.origin, FrameOrigin::Fresh);
    assert_eq!(acquired.frame.pixels.as_ref(), first_pixels.as_slice());

    provider.last_live_at = Some(Instant::now() - LAST_FRAME_HOLD);
    let acquired = provider.acquire();
    assert_eq!(acquired.origin, FrameOrigin::Cached);
    assert_eq!(
        acquired.frame.pixels.as_ref(),
        first_pixels.as_slice(),
        "a live producer with an unchanged sequence is not disconnected"
    );
    provider.last_live_at = Some(Instant::now());

    drop(first_producer);
    provider.next_generation_probe = Instant::now();
    assert_eq!(
        provider.acquire().frame.pixels.as_ref(),
        first_pixels.as_slice(),
        "brief generation gap keeps the last complete frame"
    );
    provider.last_live_at = Some(Instant::now() - LAST_FRAME_HOLD);
    assert_eq!(
        provider.acquire().frame.pixels.as_ref(),
        waiting_placeholder().as_slice(),
        "an extended generation gap falls back to the placeholder"
    );

    let mut second_producer = SharedFrameRingProducer::create(&ring_name, DEFAULT_MAX_FRAME_BYTES)
        .expect("second producer");
    second_producer
        .publish_nv12(
            PLACEHOLDER_WIDTH,
            PLACEHOLDER_HEIGHT,
            PLACEHOLDER_WIDTH,
            0,
            2,
            &second_pixels,
        )
        .expect("second generation frame");

    provider.next_generation_probe = Instant::now();
    let acquired = provider.acquire();
    assert_eq!(acquired.origin, FrameOrigin::Fresh);
    assert_eq!(acquired.frame.pixels.as_ref(), second_pixels.as_slice());

    drop((provider, second_producer));
    let _ = std::fs::remove_file(SharedFrameRingProducer::flink_path(&ring_name));
}

#[test]
fn background_workers_publish_latest_prepared_frames() {
    let ring_name = test_ring_name();
    let frame_len = nv12_len(1280, 720).expect("NV12 size");
    let pixels = vec![73; frame_len];
    let mut producer =
        SharedFrameRingProducer::create(&ring_name, DEFAULT_MAX_FRAME_BYTES).expect("producer");
    producer
        .publish_nv12(1280, 720, 1280, 0, 1, &pixels)
        .expect("publish");
    let provider = FrameProvider::with_ring_name(ring_name.clone()).expect("provider");
    provider.set_output_active(1280, 720, true);

    let deadline = Instant::now() + Duration::from_secs(2);
    let prepared = loop {
        let acquired = provider
            .acquire_for_output(1280, 720)
            .expect("supported output");
        if acquired.frame.pixels.as_ref() == pixels.as_slice() {
            assert_eq!(acquired.origin, FrameOrigin::Fresh);
            break acquired.frame;
        }
        assert_eq!(acquired.origin, FrameOrigin::Placeholder);
        assert!(
            Instant::now() < deadline,
            "workers did not prepare live frame"
        );
        thread::sleep(Duration::from_millis(5));
    };

    let repeated = provider
        .acquire_for_output(1280, 720)
        .expect("supported output");
    assert_eq!(repeated.origin, FrameOrigin::Cached);
    assert_eq!(
        repeated.frame.pixels.as_ptr(),
        prepared.pixels.as_ptr(),
        "RequestSample cache hits must only clone the prepared Arc"
    );
    assert_eq!(provider.preparation_counts(), (0, 1, 0));

    provider.set_output_active(1920, 1080, true);
    let second_deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let acquired = provider
            .acquire_for_output(1920, 1080)
            .expect("supported second output");
        if acquired.origin == FrameOrigin::Fresh {
            assert_eq!((acquired.frame.width, acquired.frame.height), (1920, 1080));
            break;
        }
        assert!(
            Instant::now() < second_deadline,
            "worker did not prepare the second active output"
        );
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        provider.preparation_counts(),
        (0, 1, 1),
        "adding a consumer must not reprepare an unchanged active format"
    );

    provider.set_output_active(1280, 720, false);
    provider.set_output_active(1920, 1080, false);
    let counts_after_stop = provider.preparation_counts();
    thread::sleep(Duration::from_millis(
        4 * RING_POLL_INTERVAL.as_millis() as u64,
    ));
    assert_eq!(provider.preparation_counts(), counts_after_stop);

    provider.shutdown();
    drop(producer);
    let _ = std::fs::remove_file(SharedFrameRingProducer::flink_path(&ring_name));
}

#[test]
fn worker_shutdown_is_idempotent_and_keeps_last_prepared_frame_readable() {
    let provider =
        FrameProvider::with_ring_name(test_ring_name()).expect("isolated frame provider");
    let before = provider
        .acquire_for_output(1280, 720)
        .expect("supported output");

    provider.shutdown();
    provider.shutdown();

    let after = provider
        .acquire_for_output(1280, 720)
        .expect("prepared cache remains valid");
    assert_eq!(after.origin, FrameOrigin::Placeholder);
    assert_eq!(after.frame.pixels.as_ptr(), before.frame.pixels.as_ptr());
}

#[test]
fn negotiated_output_shape_is_stable_and_letterboxes_input() {
    let source = OwnedNv12Frame {
        width: 4,
        height: 4,
        stride: 4,
        pixels: {
            let mut pixels = vec![80; nv12_len(4, 4).expect("source size")];
            pixels[16..].fill(128);
            pixels.into()
        },
    };
    let fitted = fit_nv12(&source, 8, 4, &mut PreparationResources::default()).expect("fit");
    assert_eq!((fitted.width, fitted.height, fitted.stride), (8, 4, 8));
    assert_eq!(fitted.pixels[0], 0, "left pillar is black");
    assert_eq!(fitted.pixels[2], 80, "source is centered");
    assert_eq!(fitted.pixels[7], 0, "right pillar is black");
}
