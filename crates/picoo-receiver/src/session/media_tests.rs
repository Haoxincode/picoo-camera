//! Receiver media admission and stream-gate regressions.

use super::*;
#[cfg(not(target_os = "macos"))]
use crate::session::media_publish::FrameTimeline;
use bytes::Bytes;
#[cfg(not(target_os = "macos"))]
use picoo_media_decode::DecodedFrame;
use picoo_media_decode::StubDecoder;
use picoo_protocol::control::StreamConfig;
use picoo_protocol::VideoPacketFlags;

fn receiver_for_generation(generation: u32) -> ReceiverSession {
    let mut receiver = ReceiverSession::new();
    receiver.decoder_worker = DecoderWorker::with_decoder(Box::new(StubDecoder::new()));
    receiver.control_generation = Some(1);
    receiver.current_stream_config = Some(Arc::new(StreamConfig {
        codec: picoo_protocol::control::VideoCodec::Avc as i32,
        width: 1280,
        height: 720,
        fps: 30,
        stream_epoch: generation,
        ..Default::default()
    }));
    receiver
}

#[test]
fn complete_refresh_supersedes_an_older_unresolved_gap() {
    for already_awaiting in [false, true] {
        let mut receiver = receiver_for_generation(1);
        if already_awaiting {
            receiver
                .enter_decoder_recovery(RecoveryReason::InitialConfig, true)
                .unwrap();
        }
        receiver.set_permit_unpaired_video(true);
        receiver
            .ingest_video_packet(packet(1, 1, true, 0, 1), Instant::now())
            .unwrap();
        receiver.jitter.discard_queued();
        // Frame 2 never arrives. Frame 3 is already a complete random-access candidate.
        receiver
            .ingest_video_packet(packet(1, 3, true, 0, 1), Instant::now())
            .unwrap();
        assert_eq!(receiver.reassembly.oldest_unresolved_frame_id(), None);
        assert_eq!(receiver.jitter.front_frame_id(), Some(3));
        assert!(receiver.awaiting_decoder_refresh_for_test());
        // The cut is terminal for late old fragments, but does not mean the Decoder
        // has accepted this candidate. Completion still owns recovery confirmation.
        receiver
            .ingest_video_packet(packet(1, 2, false, 0, 1), Instant::now())
            .unwrap();
        assert_eq!(receiver.jitter.front_frame_id(), Some(3));
        assert_eq!(receiver.ingress.decoded_frames, 0);
    }
}

#[test]
fn delta_and_incomplete_refresh_cannot_cut_an_older_gap() {
    let mut receiver = receiver_for_generation(1);
    receiver.set_permit_unpaired_video(true);
    receiver
        .ingest_video_packet(packet(1, 1, true, 0, 1), Instant::now())
        .unwrap();
    receiver.jitter.discard_queued();
    receiver
        .ingest_video_packet(packet(1, 3, false, 0, 1), Instant::now())
        .unwrap();
    receiver
        .ingest_video_packet(packet(1, 4, true, 0, 2), Instant::now())
        .unwrap();
    assert_eq!(receiver.reassembly.oldest_unresolved_frame_id(), Some(2));
    assert!(!receiver.awaiting_decoder_refresh_for_test());
    receiver
        .ingest_video_packet(packet(1, 4, true, 1, 2), Instant::now())
        .unwrap();
    assert_eq!(receiver.reassembly.oldest_unresolved_frame_id(), None);
    assert_eq!(receiver.jitter.front_frame_id(), Some(4));
    assert!(receiver.awaiting_decoder_refresh_for_test());
}

fn access_unit(generation: u64, frame_id: u64) -> EncodedAccessUnit {
    access_unit_with_kind(generation, frame_id, FrameKind::Key)
}

fn access_unit_with_kind(generation: u64, frame_id: u64, kind: FrameKind) -> EncodedAccessUnit {
    EncodedAccessUnit {
        connection_generation: 1,
        stream_generation: generation,
        frame_id,
        source_pts_us: 42_000,
        encoded_at_us: 45_000,
        received_at_us: 50_000,
        decode_submitted_at_us: 55_000,
        kind,
        data: Bytes::from(vec![frame_id as u8]),
    }
}

struct RecoveryBlockingDecoder {
    started: Arc<std::sync::atomic::AtomicBool>,
    release: Arc<std::sync::atomic::AtomicBool>,
    submitted: Arc<std::sync::Mutex<Vec<u8>>>,
}

impl picoo_media_decode::AccessUnitDecoder for RecoveryBlockingDecoder {
    fn submit(
        &mut self,
        submission: picoo_media_decode::DecodeSubmission<'_>,
    ) -> Result<picoo_media_decode::DecodeOutcome, picoo_media_decode::DecodeError> {
        let access_unit = submission.access_unit;
        let _stream_config = submission.token.stream_config.as_deref();

        let marker = access_unit.first().copied().unwrap_or(0);
        self.submitted
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(marker);
        self.started
            .store(true, std::sync::atomic::Ordering::Release);
        while !self.release.load(std::sync::atomic::Ordering::Acquire) {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        Ok(picoo_media_decode::DecodeOutcome::accepted_without_frame(
            marker == 100,
        ))
    }

    fn reset(&mut self) -> Result<(), picoo_media_decode::DecodeError> {
        Ok(())
    }
}

fn push_reference(receiver: &mut ReceiverSession, frame_id: u64) {
    assert_eq!(
        receiver.jitter.push_at(
            JitterFrame {
                stream_generation: 2,
                frame_id,
                pts_us: frame_id * 1_000,
                encoded_at_us: frame_id * 1_000,
                received_at_us: frame_id * 1_000,
                data: Bytes::from(vec![frame_id as u8]),
                keyframe: false,
                discardable: false,
            },
            0,
            0,
        ),
        PushOutcome::Accepted
    );
}

fn packet(
    generation: u32,
    frame_id: u64,
    keyframe: bool,
    fragment_index: u16,
    fragment_count: u16,
) -> VideoPacket {
    let mut flags = VideoPacketFlags::empty();
    if keyframe {
        flags |= VideoPacketFlags::KEYFRAME;
    }
    if fragment_index == 0 {
        flags |= VideoPacketFlags::START_OF_ACCESS_UNIT;
    }
    if fragment_index + 1 == fragment_count {
        flags |= VideoPacketFlags::END_OF_ACCESS_UNIT;
    }
    VideoPacket {
        flags,
        stream_epoch: generation,
        frame_id,
        pts_us: frame_id * 1_000,
        encoded_at_us: frame_id * 1_000,
        fragment_index,
        fragment_count,
        payload: Bytes::from(vec![frame_id as u8]),
    }
}

#[test]
fn stale_generation_never_reaches_decoder_or_latest_store() {
    let mut receiver = receiver_for_generation(2);

    receiver
        .publish_timeline_access_unit(access_unit(1, 9))
        .expect("stale completion is an expected drop");

    assert_eq!(receiver.ingress.decode_invocations, 0);
    assert_eq!(receiver.ingress.recovery_dropped_access_units, 1);
    assert!(receiver.frames.latest().is_none());
}

#[test]
fn matching_generation_preserves_access_unit_timeline() {
    let mut receiver = receiver_for_generation(2);

    receiver
        .publish_timeline_access_unit(access_unit(2, 9))
        .expect("matching generation");
    receiver.drain_decoder_until_idle_for_test();

    let frame = receiver.frames.latest().expect("video frame");
    #[cfg(not(target_os = "macos"))]
    {
        assert_eq!(frame.stream_generation, 2);
        assert_eq!(frame.frame_id, 9);
        assert_eq!(frame.source_pts_us, 42_000);
        assert_eq!(frame.received_at_us, 50_000);
    }
    #[cfg(target_os = "macos")]
    {
        assert_eq!(frame.identity().stream_epoch, 2);
        assert_eq!(frame.identity().frame_id, 9);
        assert_eq!(frame.source_pts_us(), 42_000);
        assert_eq!(frame.timeline().received_at_us, 50_000);
    }
}

#[test]
fn ready_reference_waits_in_jitter_until_decoder_capacity_is_available() {
    use std::sync::atomic::{AtomicBool, Ordering};

    struct BlockingDecoder {
        started: Arc<AtomicBool>,
        release: Arc<AtomicBool>,
    }

    impl picoo_media_decode::AccessUnitDecoder for BlockingDecoder {
        fn submit(
            &mut self,
            submission: picoo_media_decode::DecodeSubmission<'_>,
        ) -> Result<picoo_media_decode::DecodeOutcome, picoo_media_decode::DecodeError> {
            let _access_unit = submission.access_unit;
            let _stream_config = submission.token.stream_config.as_deref();

            self.started.store(true, Ordering::Release);
            while !self.release.load(Ordering::Acquire) {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Ok(picoo_media_decode::DecodeOutcome::accepted_without_frame(
                false,
            ))
        }

        fn reset(&mut self) -> Result<(), picoo_media_decode::DecodeError> {
            Ok(())
        }
    }

    let started = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let mut receiver = receiver_for_generation(2);
    receiver.decoder_worker = DecoderWorker::with_decoder(Box::new(BlockingDecoder {
        started: Arc::clone(&started),
        release: Arc::clone(&release),
    }));
    receiver.jitter.set_fixed_target_ms(Some(0));
    receiver
        .publish_timeline_access_unit(access_unit(2, 1))
        .expect("start decoder");
    let start_deadline = Instant::now() + std::time::Duration::from_secs(1);
    while !started.load(Ordering::Acquire) && Instant::now() < start_deadline {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(started.load(Ordering::Acquire), "decoder never started");
    receiver
        .publish_timeline_access_unit(access_unit_with_kind(2, 2, FrameKind::ReferenceDelta))
        .expect("first pending reference");
    receiver
        .publish_timeline_access_unit(access_unit_with_kind(2, 3, FrameKind::ReferenceDelta))
        .expect("second pending reference");
    assert_eq!(
        receiver.jitter.push_at(
            JitterFrame {
                stream_generation: 2,
                frame_id: 4,
                pts_us: 4_000,
                encoded_at_us: 4_000,
                received_at_us: 4_000,
                data: Bytes::from_static(b"waiting-reference"),
                keyframe: false,
                discardable: false,
            },
            0,
            0,
        ),
        PushOutcome::Accepted
    );

    receiver.drain_jitter().expect("capacity-aware drain");
    assert_eq!(receiver.jitter.len(), 1, "ready AU left jitter too early");
    assert!(!receiver.decoder_recovery.awaiting_refresh());

    release.store(true, Ordering::Release);
    let drain_deadline = Instant::now() + std::time::Duration::from_secs(1);
    while !receiver.jitter.is_empty() && Instant::now() < drain_deadline {
        receiver.drain_decoder_events().expect("decoder events");
        receiver.drain_jitter().expect("capacity released");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(
        receiver.jitter.is_empty(),
        "waiting AU never reached decoder"
    );
    assert!(!receiver.decoder_recovery.awaiting_refresh());
}

#[test]
fn recovery_idr_completion_releases_following_references_in_order() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let started = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let submitted = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut receiver = receiver_for_generation(2);
    receiver.decoder_worker = DecoderWorker::with_decoder(Box::new(RecoveryBlockingDecoder {
        started: Arc::clone(&started),
        release: Arc::clone(&release),
        submitted: Arc::clone(&submitted),
    }));
    receiver.jitter.set_fixed_target_ms(Some(0));
    receiver
        .enter_decoder_recovery(RecoveryReason::DecoderError, true)
        .expect("enter recovery");
    receiver
        .publish_timeline_access_unit(access_unit(2, 100))
        .expect("submit recovery IDR");

    let start_deadline = Instant::now() + std::time::Duration::from_secs(1);
    while !started.load(Ordering::Acquire) && Instant::now() < start_deadline {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(
        started.load(Ordering::Acquire),
        "recovery IDR never started"
    );

    push_reference(&mut receiver, 101);
    push_reference(&mut receiver, 102);
    receiver.drain_jitter().expect("wait for IDR completion");
    assert_eq!(receiver.jitter.len(), 2);
    assert!(receiver.decoder_recovery.awaiting_refresh());

    release.store(true, Ordering::Release);
    let completion_deadline = Instant::now() + std::time::Duration::from_secs(1);
    loop {
        receiver.drain_decoder_events().expect("decoder events");
        receiver.drain_jitter().expect("release recovered chain");
        let submitted_len = submitted
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len();
        if submitted_len == 3 || Instant::now() >= completion_deadline {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    assert_eq!(
        *submitted
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        vec![100, 101, 102]
    );
    assert!(receiver.jitter.is_empty());
    assert!(!receiver.decoder_recovery.awaiting_refresh());
}

#[test]
fn reference_loss_invalidates_in_flight_idr_completion() {
    use std::sync::atomic::{AtomicBool, Ordering};

    let started = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let submitted = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut receiver = receiver_for_generation(2);
    receiver.decoder_worker = DecoderWorker::with_decoder(Box::new(RecoveryBlockingDecoder {
        started: Arc::clone(&started),
        release: Arc::clone(&release),
        submitted,
    }));
    receiver.jitter.set_fixed_target_ms(Some(0));
    receiver
        .enter_decoder_recovery(RecoveryReason::DecoderError, true)
        .expect("enter recovery");
    receiver
        .publish_timeline_access_unit(access_unit(2, 100))
        .expect("submit recovery IDR");

    let start_deadline = Instant::now() + std::time::Duration::from_secs(1);
    while !started.load(Ordering::Acquire) && Instant::now() < start_deadline {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(
        started.load(Ordering::Acquire),
        "recovery IDR never started"
    );
    push_reference(&mut receiver, 101);
    receiver.drain_jitter().expect("wait for IDR completion");

    receiver
        .enter_decoder_recovery(RecoveryReason::ReferenceAccessUnitLost, true)
        .expect("invalidate recovery candidate");
    assert!(receiver.jitter.is_empty());
    release.store(true, Ordering::Release);
    receiver.drain_decoder_until_idle_for_test();

    assert!(receiver.decoder_recovery.awaiting_refresh());
}

#[test]
fn stale_connection_generation_cannot_publish_into_current_stream() {
    let mut receiver = receiver_for_generation(2);
    receiver.control_generation = Some(8);
    let timeline = AccessUnitTimeline {
        connection_generation: 7,
        stream_generation: 2,
        frame_id: 9,
        source_pts_us: 42_000,
        encoded_at_us: 45_000,
        received_at_us: 50_000,
        decode_submitted_at_us: 55_000,
        kind: FrameKind::Key,
    };

    assert!(!receiver.decoder_timeline_is_current(timeline));
}

#[test]
fn zero_generation_fixture_cannot_bypass_current_timeline() {
    let receiver = receiver_for_generation(2);
    let timeline = AccessUnitTimeline {
        connection_generation: 0,
        stream_generation: 0,
        frame_id: 9,
        source_pts_us: 42_000,
        encoded_at_us: 45_000,
        received_at_us: 50_000,
        decode_submitted_at_us: 55_000,
        kind: FrameKind::Key,
    };

    assert!(!receiver.decoder_timeline_is_current(timeline));
}

#[cfg(not(target_os = "macos"))]
#[test]
fn receiver_reuses_transformed_pixels_after_latest_frame_releases_them() {
    let mut receiver = receiver_for_generation(2);
    for frame_id in 1..=3 {
        receiver
            .publish_decoded_frame(
                FrameTimeline {
                    stream_generation: 2,
                    frame_id,
                    source_pts_us: frame_id * 1_000,
                    encoded_at_us: frame_id * 1_050,
                    received_at_us: frame_id * 1_100,
                    decode_submitted_at_us: frame_id * 1_150,
                    decoded_at: Some(Instant::now()),
                },
                DecodedFrame::cpu_nv12(
                    4,
                    2,
                    4,
                    90,
                    frame_id * 1_200,
                    Bytes::from_static(&[1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 20, 21]),
                ),
                false,
            )
            .expect("publish transformed frame");
    }

    let stats = receiver.frame_buffer_pool.stats();
    assert_eq!(stats.allocations, 2);
    assert_eq!(stats.reuses, 1);
    assert_eq!(stats.retained_buffers, 1);
    assert_eq!(receiver.latest_frame().map(|frame| frame.frame_id), Some(3));
    assert_eq!(receiver.ingress.orientation_transform_frames, 3);
}

#[test]
fn future_idr_waits_for_matching_stream_config() {
    let mut receiver = receiver_for_generation(1);
    receiver.set_permit_unpaired_video(true);

    receiver
        .ingest_video_packet(packet(2, 7, true, 0, 1), Instant::now())
        .expect("future IDR");

    assert_eq!(receiver.waiting_for_stream_config_epoch, Some(2));
    assert_eq!(
        receiver
            .pending_stream_config_idr
            .as_ref()
            .map(|access_unit| (access_unit.stream_epoch, access_unit.frame_id)),
        Some((2, 7))
    );
    assert!(receiver.jitter.is_empty());

    receiver
        .release_pending_stream_config_idr(2)
        .expect("release matching IDR");
    assert!(receiver.pending_stream_config_idr.is_none());
    assert_eq!(receiver.jitter.len(), 1);
}

#[test]
fn future_delta_and_incomplete_idr_never_cross_stream_config_gate() {
    let mut receiver = receiver_for_generation(1);
    receiver.set_permit_unpaired_video(true);

    receiver
        .ingest_video_packet(packet(2, 7, false, 0, 1), Instant::now())
        .expect("future delta");
    assert!(receiver.pending_stream_config_idr.is_none());

    receiver
        .ingest_video_packet(packet(3, 8, true, 0, 2), Instant::now())
        .expect("partial future IDR");
    assert_eq!(receiver.waiting_for_stream_config_epoch, Some(3));
    assert!(receiver.pending_stream_config_idr.is_none());
    assert!(receiver.jitter.is_empty());
}

#[test]
fn newest_future_epoch_supersedes_older_gate_without_crossing_generations() {
    let mut receiver = receiver_for_generation(1);
    receiver.set_permit_unpaired_video(true);

    receiver
        .ingest_video_packet(packet(2, 7, true, 0, 1), Instant::now())
        .expect("epoch two IDR");
    receiver
        .ingest_video_packet(packet(3, 8, true, 0, 1), Instant::now())
        .expect("epoch three IDR");
    receiver
        .ingest_video_packet(packet(2, 9, true, 0, 1), Instant::now())
        .expect("late older IDR");

    assert_eq!(receiver.waiting_for_stream_config_epoch, Some(3));
    assert_eq!(
        receiver
            .pending_stream_config_idr
            .as_ref()
            .map(|access_unit| (access_unit.stream_epoch, access_unit.frame_id)),
        Some((3, 8))
    );

    receiver
        .release_pending_stream_config_idr(2)
        .expect("nonmatching config is harmless");
    assert_eq!(
        receiver
            .pending_stream_config_idr
            .as_ref()
            .map(|access_unit| access_unit.stream_epoch),
        Some(3)
    );
    assert!(receiver.jitter.is_empty());
}

#[test]
fn stale_transport_batch_discards_whole_access_unit_and_blocks_late_tail() {
    let mut receiver = receiver_for_generation(1);
    receiver.set_permit_unpaired_video(true);
    let batch = ReceivedVideoPacketBatch::new(
        Instant::now(),
        vec![packet(1, 7, true, 0, 2), packet(1, 7, true, 1, 2)],
    );

    receiver
        .discard_stale_video_batch(batch)
        .expect("stale access unit is an expected media drop");

    assert_eq!(receiver.ingress.receive_queue_expired_access_units, 1);
    assert_eq!(receiver.ingress.reassembly_partial_access_unit_drops, 0);
    assert_eq!(receiver.ingress.reassembly_whole_access_unit_gap_drops, 0);
    assert!(receiver.awaiting_decoder_refresh_for_test());

    receiver
        .ingest_video_packet(packet(1, 7, true, 1, 2), Instant::now())
        .expect("late tail is ignored by the terminal boundary");
    assert_eq!(receiver.ingress.access_units, 0);
    assert_eq!(receiver.ingress.decode_invocations, 0);
}

#[test]
fn teardown_discards_stream_config_gate() {
    let mut receiver = receiver_for_generation(1);
    receiver.set_permit_unpaired_video(true);
    receiver
        .ingest_video_packet(packet(2, 7, true, 0, 1), Instant::now())
        .expect("future IDR");

    receiver.close();

    assert!(receiver.waiting_for_stream_config_epoch.is_none());
    assert!(receiver.pending_stream_config_idr.is_none());
    assert!(receiver.jitter.is_empty());
}

#[test]
fn delayed_frame_keeps_submitted_rotation_and_mirror_snapshot() {
    // REQ-PICOO-MEDIA-025: completion may already be queued, but the owner has
    // not consumed it when current state changes. No timing-dependent sleep.
    for (rotation, mirrored, expected_size, expected_pixels) in [
        (
            0,
            false,
            (4, 2),
            vec![1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 20, 21],
        ),
        (
            90,
            true,
            (2, 4),
            vec![1, 5, 2, 6, 3, 7, 4, 8, 10, 11, 20, 21],
        ),
    ] {
        let mut receiver = receiver_for_generation(2);
        let submitted = Arc::make_mut(receiver.current_stream_config.as_mut().unwrap());
        submitted.width = 4;
        submitted.height = 2;
        submitted.rotation = rotation;
        submitted.mirrored = mirrored;
        let mut unit = access_unit(2, 9);
        unit.data = Bytes::from_static(&[1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 20, 21]);
        receiver.publish_timeline_access_unit(unit).unwrap();

        let current = Arc::make_mut(receiver.current_stream_config.as_mut().unwrap());
        current.rotation = if rotation == 0 { 90 } else { 0 };
        current.mirrored = !mirrored;
        receiver.drain_decoder_until_idle_for_test();

        let frame = receiver.latest_frame().expect("submitted frame");
        #[cfg(not(target_os = "macos"))]
        {
            assert_eq!((frame.width, frame.height), expected_size);
            assert_eq!(frame.pixel_data.as_ref(), expected_pixels);
            assert_eq!(frame.frame_id, 9);
            assert_eq!(frame.source_pts_us, 42_000);
        }
        #[cfg(target_os = "macos")]
        {
            let _ = (expected_size, expected_pixels);
            assert_eq!((frame.image().width(), frame.image().height()), (4, 2));
            assert_eq!(
                frame.description().transform.rotation,
                if rotation == 0 {
                    picoo_frame_hub::Rotation::None
                } else {
                    picoo_frame_hub::Rotation::Clockwise90
                }
            );
            assert_eq!(frame.description().transform.mirror, mirrored);
            assert_eq!(frame.identity().frame_id, 9);
            assert_eq!(frame.source_pts_us(), 42_000);
            assert_eq!(receiver.ingress.orientation_transform_frames, 0);
        }
    }
}

#[test]
fn placeholder_does_not_inherit_source_mirror() {
    let mut receiver = receiver_for_generation(2);
    Arc::make_mut(receiver.current_stream_config.as_mut().unwrap()).mirrored = true;
    receiver.publish_waiting_placeholder().unwrap();
    #[cfg(not(target_os = "macos"))]
    {
        let expected = receiver.placeholder_mode.waiting_frame();
        assert_eq!(
            receiver.latest_frame().unwrap().pixel_data.as_ref(),
            expected
        );
    }
    #[cfg(target_os = "macos")]
    assert!(
        receiver.latest_frame().is_none(),
        "placeholder is an output, never a source frame"
    );
}
