use super::*;
use crate::{
    ChromaSiting, FrameDescription, FrameTimeline, ImageSize, NativeImage, PixelAspectRatio,
    PresentationTransform, Rotation, SourceColor, VisibleRect,
};

fn description() -> FrameDescription {
    FrameDescription {
        coded_size: ImageSize {
            width: 1280,
            height: 720,
        },
        visible_rect: VisibleRect {
            x: 0,
            y: 0,
            width: 1280,
            height: 720,
        },
        pixel_aspect_ratio: PixelAspectRatio {
            numerator: 1,
            denominator: 1,
        },
        color: SourceColor::Nv12Bt709Limited {
            chroma_siting: ChromaSiting::Left,
        },
        transform: PresentationTransform {
            rotation: Rotation::None,
            mirror: false,
        },
        config_revision: 5,
    }
}

fn frame(id: u64) -> NativeVideoFrame {
    make_frame(id, description()).unwrap()
}

fn make_frame(
    id: u64,
    description: FrameDescription,
) -> Result<NativeVideoFrame, crate::InvalidFrameDescription> {
    NativeVideoFrame::new(
        FrameIdentity {
            connection_generation: 2,
            stream_epoch: 3,
            decoder_generation: 4,
            frame_id: id,
        },
        id * 16_667,
        description,
        NativeImage::Fake {
            width: 1280,
            height: 720,
        },
        FrameTimeline {
            encoded_at_us: 10,
            received_at_us: 20,
            decode_submitted_at_us: 30,
            decoded_at: Instant::now(),
        },
    )
}

#[test]
fn preview_and_ordered_subscription_share_the_immutable_original() {
    let mut bus = FrameBus::new();
    bus.publish(frame(1));
    let old_preview = Arc::clone(bus.latest().unwrap());
    let mut ordered = bus.subscribe_ordered().unwrap();
    assert!(ordered.try_next().unwrap().is_none());
    bus.publish(frame(2));
    let received = ordered.try_next().unwrap().unwrap();
    assert!(Arc::ptr_eq(&received, bus.latest().unwrap()));
    assert_eq!(old_preview.identity().frame_id, 1);
    assert_eq!(
        received.identity(),
        FrameIdentity {
            connection_generation: 2,
            stream_epoch: 3,
            decoder_generation: 4,
            frame_id: 2
        }
    );
    assert_eq!(received.description().config_revision, 5);
    assert_eq!(received.source_pts_us(), 33_334);
}

#[test]
fn stalled_recorder_ends_explicitly_while_latest_keeps_advancing() {
    let mut bus = FrameBus::new();
    let mut ordered = bus.subscribe_ordered().unwrap();
    for id in 0..8 {
        assert_eq!(bus.publish(frame(id)), None);
    }
    let failed = frame(8).identity();
    assert_eq!(
        bus.publish(frame(8)),
        Some(SubscriptionEnd::QueueFull(failed))
    );
    for id in 9..1000 {
        assert_eq!(bus.publish(frame(id)), None);
    }
    assert_eq!(bus.latest().unwrap().identity().frame_id, 999);
    assert!(matches!(ordered.try_next(), Err(SubscriptionEnd::QueueFull(id)) if id == failed));
    // Failed subscriptions do not replay stale queued data or revive silently.
    assert!(matches!(
        ordered.try_next(),
        Err(SubscriptionEnd::QueueFull(_))
    ));
    assert!(bus.subscribe_ordered().is_ok());
}

#[test]
fn ordered_queue_preserves_frame_order_and_rejects_expired_work() {
    let mut bus = FrameBus::new();
    let mut ordered = bus.subscribe_ordered().unwrap();
    let now = Instant::now();
    for id in 1..4 {
        bus.publish_at(frame(id), now);
    }
    assert_eq!(
        ordered
            .try_next_at(now)
            .unwrap()
            .unwrap()
            .identity()
            .frame_id,
        1
    );
    assert_eq!(
        ordered
            .try_next_at(now)
            .unwrap()
            .unwrap()
            .identity()
            .frame_id,
        2
    );
    let expired = frame(3).identity();
    assert!(
        matches!(ordered.try_next_at(now + Duration::from_millis(151)), Err(SubscriptionEnd::TooOld(id)) if id == expired)
    );
    assert_eq!(bus.latest().unwrap().identity().frame_id, 3);
}

#[test]
fn clear_and_publisher_drop_cannot_look_like_successful_end_of_recording() {
    let mut bus = FrameBus::new();
    let mut ordered = bus.subscribe_ordered().unwrap();
    bus.publish(frame(1));
    bus.clear();
    assert!(bus.latest().is_none());
    assert!(matches!(ordered.try_next(), Err(SubscriptionEnd::Reset)));
    let mut ordered = bus.subscribe_ordered().unwrap();
    bus.publish(frame(2));
    drop(bus);
    assert!(matches!(
        ordered.try_next(),
        Err(SubscriptionEnd::PublisherStopped)
    ));
}

#[test]
fn only_one_recorder_subscription_is_active_at_a_time() {
    let mut bus = FrameBus::new();
    let ordered = bus.subscribe_ordered().unwrap();
    assert!(bus.subscribe_ordered().is_err());
    drop(ordered);
    assert!(bus.subscribe_ordered().is_ok());
}

#[test]
fn native_geometry_rejects_overflow_invalid_crop_and_zero_aspect() {
    for bad in [
        FrameDescription {
            visible_rect: VisibleRect {
                x: u32::MAX - 1,
                y: 0,
                width: 4,
                height: 720,
            },
            ..description()
        },
        FrameDescription {
            visible_rect: VisibleRect {
                x: 0,
                y: 0,
                width: 1282,
                height: 720,
            },
            ..description()
        },
        FrameDescription {
            visible_rect: VisibleRect {
                x: 1,
                y: 0,
                width: 1278,
                height: 720,
            },
            ..description()
        },
        FrameDescription {
            pixel_aspect_ratio: PixelAspectRatio {
                numerator: 1,
                denominator: 0,
            },
            ..description()
        },
    ] {
        assert!(make_frame(1, bad).is_err());
    }
}
