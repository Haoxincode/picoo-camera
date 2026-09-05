//! LatestFrameStore, Shared Frame Ring, placeholders, and H.264 decode publish.
//!
//! REQ-PICOO-FRAME-*, REQ-PICOO-MEDIA-004/006/009/017/023.

use bytes::Bytes;
use picoo_frame_hub::{PlaceholderMode, PLACEHOLDER_HEIGHT, PLACEHOLDER_WIDTH};
use picoo_jitter::{Frame as JitterFrame, PushOutcome};
use picoo_media_decode::DecodedFrame;
use picoo_packet::AssembledAccessUnit;
#[cfg(test)]
use picoo_protocol::VideoPacket;
#[cfg(test)]
use picoo_transport::ReceivedVideoPacketBatch;
#[cfg(test)]
use std::sync::Arc;
use std::time::Instant;

#[cfg(test)]
use super::decoder_worker::{AccessUnitTimeline, DecoderWorker, FrameKind};
use super::decoder_worker::{DecodeSubmitOutcome, EncodedAccessUnit};
use super::media_publish::FrameTimeline;
use super::recovery::RecoveryReason;
use super::ReceiverSession;
use crate::{ReceiverError, DEFAULT_SHARED_RING_NAME};

#[cfg(test)]
mod tests {
    use super::*;
    use picoo_media_decode::StubDecoder;
    use picoo_protocol::control::StreamConfig;
    use picoo_protocol::VideoPacketFlags;

    fn receiver_for_generation(generation: u32) -> ReceiverSession {
        let mut receiver = ReceiverSession::new();
        receiver.decoder_worker = DecoderWorker::with_decoder(Box::new(StubDecoder::new()));
        receiver.control_generation = Some(1);
        receiver.current_stream_config = Some(Arc::new(StreamConfig {
            codec: "h264".into(),
            width: 1280,
            height: 720,
            fps: 30,
            stream_epoch: generation,
            ..Default::default()
        }));
        receiver
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
            data: Bytes::from_static(b"typed-au"),
        }
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
        assert!(receiver.latest_frame_store.latest().is_none());
    }

    #[test]
    fn matching_generation_preserves_access_unit_timeline() {
        let mut receiver = receiver_for_generation(2);

        receiver
            .publish_timeline_access_unit(access_unit(2, 9))
            .expect("matching generation");
        receiver.drain_decoder_until_idle_for_test();

        let frame = receiver.latest_frame_store.latest().expect("video frame");
        assert_eq!(frame.stream_generation, 2);
        assert_eq!(frame.frame_id, 9);
        assert_eq!(frame.source_pts_us, 42_000);
        assert_eq!(frame.received_at_us, 50_000);
    }

    #[test]
    fn ready_reference_waits_in_jitter_until_decoder_capacity_is_available() {
        use std::sync::atomic::{AtomicBool, Ordering};

        struct BlockingDecoder {
            started: Arc<AtomicBool>,
            release: Arc<AtomicBool>,
        }

        impl picoo_media_decode::AccessUnitDecoder for BlockingDecoder {
            fn decode_access_unit(
                &mut self,
                _access_unit: &[u8],
                _stream_config: Option<&picoo_protocol::control::StreamConfig>,
            ) -> Result<picoo_media_decode::DecodeOutcome, picoo_media_decode::DecodeError>
            {
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
}

impl ReceiverSession {
    pub(super) fn release_pending_stream_config_idr(
        &mut self,
        stream_epoch: u32,
    ) -> Result<(), ReceiverError> {
        if !self
            .pending_stream_config_idr
            .as_ref()
            .is_some_and(|access_unit| access_unit.stream_epoch == stream_epoch)
        {
            return Ok(());
        }
        let access_unit = self
            .pending_stream_config_idr
            .take()
            .expect("matching pending StreamConfig IDR exists");
        self.queue_assembled_access_unit(access_unit)?;
        Ok(())
    }

    pub(super) fn queue_assembled_access_unit(
        &mut self,
        access_unit: AssembledAccessUnit,
    ) -> Result<(), ReceiverError> {
        let pts_us = access_unit.pts_us;
        let completed_at = Instant::now();
        if access_unit.keyframe {
            tracing::warn!(
                bytes = access_unit.data.len(),
                fragments = access_unit.fragment_count,
                assembly_ms = completed_at
                    .saturating_duration_since(access_unit.first_fragment_at)
                    .as_secs_f64()
                    * 1_000.0,
                "complete keyframe reached receiver reassembly"
            );
        }
        self.interarrival_jitter.observe(completed_at, pts_us);
        let first_fragment_at_us = access_unit
            .first_fragment_at
            .saturating_duration_since(self.timing_origin)
            .as_micros() as u64;
        let completed_at_us = completed_at
            .saturating_duration_since(self.timing_origin)
            .as_micros() as u64;
        let outcome = self.jitter.push_at(
            JitterFrame {
                stream_generation: u64::from(access_unit.stream_epoch),
                frame_id: access_unit.frame_id,
                pts_us,
                encoded_at_us: access_unit.encoded_at_us,
                received_at_us: completed_at_us,
                data: access_unit.data,
                keyframe: access_unit.keyframe,
                discardable: access_unit.discardable,
            },
            first_fragment_at_us,
            completed_at_us,
        );
        match outcome {
            PushOutcome::AcceptedAfterReferenceDrop => {
                self.ingress.recovery_jitter_capacity =
                    self.ingress.recovery_jitter_capacity.saturating_add(1);
                self.enter_decoder_recovery(RecoveryReason::ReferenceAccessUnitLate, true)?
            }
            PushOutcome::DroppedLate {
                requires_refresh: true,
            } => {
                self.ingress.recovery_arrived_after_playout = self
                    .ingress
                    .recovery_arrived_after_playout
                    .saturating_add(1);
                self.enter_decoder_recovery(RecoveryReason::ReferenceAccessUnitLate, true)?
            }
            PushOutcome::Accepted
            | PushOutcome::DroppedLate {
                requires_refresh: false,
            } => {}
        }
        Ok(())
    }

    /// Attach a cross-process Shared Frame Ring for VCam consumption (REQ-PICOO-FRAME-003).
    pub fn attach_shared_ring(&mut self, name: &str) -> Result<(), ReceiverError> {
        let name = name.to_owned();
        let use_platform_ring = name == DEFAULT_SHARED_RING_NAME;
        let ring = picoo_frame_hub::SharedFrameRingWriter::start(move || {
            #[cfg(target_os = "windows")]
            if use_platform_ring {
                return picoo_frame_hub::SharedFrameRingProducer::open_or_create_file(
                    picoo_frame_hub::windows_shared_ring_path(&name),
                    picoo_frame_hub::DEFAULT_MAX_FRAME_BYTES,
                );
            }
            #[cfg(target_os = "macos")]
            if use_platform_ring {
                let path = picoo_frame_hub::macos_app_group_ring_path(&name)?;
                return picoo_frame_hub::SharedFrameRingProducer::open_or_create_file(
                    path,
                    picoo_frame_hub::DEFAULT_MAX_FRAME_BYTES,
                );
            }
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            let _ = use_platform_ring;
            picoo_frame_hub::SharedFrameRingProducer::open_or_create(
                &name,
                picoo_frame_hub::DEFAULT_MAX_FRAME_BYTES,
            )
        })?;
        self.shared_ring = Some(ring);
        self.last_shared_ring_error = None;
        self.publish_waiting_placeholder()?;
        Ok(())
    }

    pub(super) fn drain_shared_ring_events(&mut self) {
        let Some(ring) = self.shared_ring.as_ref() else {
            return;
        };
        while let Some(event) = ring.poll_event() {
            match event {
                picoo_frame_hub::SharedRingWriterEvent::Published { .. } => {
                    self.last_shared_ring_error = None;
                }
                picoo_frame_hub::SharedRingWriterEvent::Failed { error, .. } => {
                    tracing::warn!(%error, "Shared Frame Ring output failed");
                    self.last_shared_ring_error = Some(error.to_string());
                }
            }
        }
    }

    pub fn publish_waiting_placeholder(&mut self) -> Result<(), ReceiverError> {
        let nv12 = self.placeholder_mode.waiting_frame();
        self.publish_decoded_frame(
            FrameTimeline::default(),
            DecodedFrame::cpu_nv12(
                PLACEHOLDER_WIDTH,
                PLACEHOLDER_HEIGHT,
                PLACEHOLDER_WIDTH,
                0,
                0,
                Bytes::from(nv12),
            ),
        )
    }

    /// Publish reconnect-branded placeholder (REQ-PICOO-FRAME-005).
    pub fn publish_reconnecting_placeholder(&mut self) -> Result<(), ReceiverError> {
        let nv12 = self.placeholder_mode.reconnecting_frame();
        self.publish_decoded_frame(
            FrameTimeline::default(),
            DecodedFrame::cpu_nv12(
                PLACEHOLDER_WIDTH,
                PLACEHOLDER_HEIGHT,
                PLACEHOLDER_WIDTH,
                0,
                0,
                Bytes::from(nv12),
            ),
        )
    }

    pub fn set_placeholder_mode(&mut self, mode: PlaceholderMode) {
        self.placeholder_mode = mode;
    }

    pub fn placeholder_mode(&self) -> PlaceholderMode {
        self.placeholder_mode
    }

    /// Decode one typed H.264 access unit into one shared VideoFrame.
    pub(super) fn publish_timeline_access_unit(
        &mut self,
        access_unit: EncodedAccessUnit,
    ) -> Result<(), ReceiverError> {
        self.ingress.access_units += 1;
        if self
            .current_stream_config
            .as_ref()
            .is_some_and(|config| u64::from(config.stream_epoch) != access_unit.stream_generation)
        {
            self.ingress.recovery_dropped_access_units =
                self.ingress.recovery_dropped_access_units.saturating_add(1);
            return Ok(());
        }
        if !self.accepts_access_unit_for_decode(access_unit.kind.is_keyframe()) {
            self.ingress.recovery_dropped_access_units =
                self.ingress.recovery_dropped_access_units.saturating_add(1);
            return Ok(());
        }
        match self
            .decoder_worker
            .submit(access_unit, self.current_stream_config.clone())
        {
            DecodeSubmitOutcome::Queued => {}
            DecodeSubmitOutcome::Dropped { requires_refresh } => {
                self.ingress.recovery_dropped_access_units =
                    self.ingress.recovery_dropped_access_units.saturating_add(1);
                if requires_refresh {
                    self.enter_decoder_recovery(RecoveryReason::DecoderQueuePressure, true)?;
                }
            }
        }
        Ok(())
    }
}
