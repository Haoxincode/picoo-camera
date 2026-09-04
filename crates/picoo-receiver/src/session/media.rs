//! LatestFrameStore, Shared Frame Ring, placeholders, and H.264 decode publish.
//!
//! REQ-PICOO-FRAME-*, REQ-PICOO-MEDIA-004/006/009.

use bytes::Bytes;
use picoo_frame_hub::{
    PlaceholderMode, SharedFrameRingProducer, VideoFrame, PLACEHOLDER_HEIGHT, PLACEHOLDER_WIDTH,
};
use picoo_jitter::{Frame as JitterFrame, PushOutcome};
#[cfg(test)]
use picoo_media_decode::AccessUnitDecoder;
use picoo_media_decode::DecodedFrame;
use picoo_packet::{AssembledAccessUnit, ReassemblyError};
use picoo_protocol::VideoPacket;
use std::sync::Arc;
use std::time::Instant;

use super::decoder_worker::{
    AccessUnitTimeline, DecodeSubmitOutcome, DecoderEvent, EncodedAccessUnit,
};
#[cfg(test)]
use super::decoder_worker::{DecoderWorker, FrameKind};
use super::recovery::RecoveryReason;
use super::ReceiverSession;
use crate::ReceiverError;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use crate::DEFAULT_SHARED_RING_NAME;

#[cfg(test)]
mod tests {
    use super::*;
    use picoo_media_decode::StubDecoder;
    use picoo_protocol::control::StreamConfig;
    use picoo_protocol::VideoPacketFlags;

    fn receiver_for_generation(generation: u32) -> ReceiverSession {
        let mut receiver = ReceiverSession::new();
        receiver.decoder_worker = DecoderWorker::with_decoder(Box::new(StubDecoder::new()));
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
        EncodedAccessUnit {
            connection_generation: 0,
            stream_generation: generation,
            frame_id,
            source_pts_us: 42_000,
            received_at_us: 50_000,
            kind: FrameKind::Key,
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
    fn stale_connection_generation_cannot_publish_into_current_stream() {
        let mut receiver = receiver_for_generation(2);
        receiver.control_generation = Some(8);
        let timeline = AccessUnitTimeline {
            connection_generation: 7,
            stream_generation: 2,
            frame_id: 9,
            source_pts_us: 42_000,
            received_at_us: 50_000,
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
                        received_at_us: frame_id * 1_100,
                        decoded_at: Some(Instant::now()),
                    },
                    DecodedFrame {
                        width: 4,
                        height: 2,
                        stride: 4,
                        rotation: 90,
                        timestamp_us: frame_id * 1_200,
                        nv12: Bytes::from_static(&[1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 20, 21]),
                    },
                )
                .expect("publish transformed frame");
        }

        let stats = receiver.frame_buffer_pool.stats();
        assert_eq!(stats.allocations, 2);
        assert_eq!(stats.reuses, 1);
        assert_eq!(stats.retained_buffers, 1);
        assert_eq!(receiver.latest_frame().map(|frame| frame.frame_id), Some(3));
    }

    #[test]
    fn future_idr_waits_for_matching_stream_config() {
        let mut receiver = receiver_for_generation(1);
        receiver.set_permit_unpaired_video(true);

        receiver
            .ingest_video_packet(packet(2, 7, true, 0, 1))
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
            .ingest_video_packet(packet(2, 7, false, 0, 1))
            .expect("future delta");
        assert!(receiver.pending_stream_config_idr.is_none());

        receiver
            .ingest_video_packet(packet(3, 8, true, 0, 2))
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
            .ingest_video_packet(packet(2, 7, true, 0, 1))
            .expect("epoch two IDR");
        receiver
            .ingest_video_packet(packet(3, 8, true, 0, 1))
            .expect("epoch three IDR");
        receiver
            .ingest_video_packet(packet(2, 9, true, 0, 1))
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
    fn teardown_discards_stream_config_gate() {
        let mut receiver = receiver_for_generation(1);
        receiver.set_permit_unpaired_video(true);
        receiver
            .ingest_video_packet(packet(2, 7, true, 0, 1))
            .expect("future IDR");

        receiver.close();

        assert!(receiver.waiting_for_stream_config_epoch.is_none());
        assert!(receiver.pending_stream_config_idr.is_none());
        assert!(receiver.jitter.is_empty());
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct FrameTimeline {
    stream_generation: u64,
    frame_id: u64,
    source_pts_us: u64,
    received_at_us: u64,
    decoded_at: Option<Instant>,
}

impl ReceiverSession {
    pub(super) fn ingest_video_packet(&mut self, packet: VideoPacket) -> Result<(), ReceiverError> {
        // Enforce the wall-clock deadline before a queued late tail gets a
        // chance to complete an already-expired AU.
        self.expire_reassembly_deadline()?;
        self.ingress.packets_received += 1;
        if !self.video_allowed() {
            self.ingress.packets_dropped_unpaired += 1;
            return Ok(());
        }

        let packet_epoch = packet.stream_epoch;
        let configured_epoch = self
            .current_stream_config
            .as_ref()
            .map(|config| config.stream_epoch);
        let (configured_epoch, mut defer_until_config) = match configured_epoch {
            Some(epoch) => (epoch, false),
            None if self.permit_unpaired_video => (packet_epoch, false),
            None => {
                match self.waiting_for_stream_config_epoch {
                    Some(waiting) if packet_epoch < waiting => return Ok(()),
                    Some(waiting) if packet_epoch == waiting => {}
                    Some(_) | None => {
                        self.pending_stream_config_idr = None;
                        self.waiting_for_stream_config_epoch = Some(packet_epoch);
                    }
                }
                (packet_epoch, true)
            }
        };
        if packet_epoch < configured_epoch {
            return Ok(());
        }
        if packet_epoch > configured_epoch {
            if self
                .waiting_for_stream_config_epoch
                .is_some_and(|waiting| packet_epoch < waiting)
            {
                return Ok(());
            }
            if self.waiting_for_stream_config_epoch != Some(packet_epoch) {
                self.pending_stream_config_idr = None;
                self.waiting_for_stream_config_epoch = Some(packet_epoch);
            }
            defer_until_config = true;
        }

        self.stats_reporter.record_packet(packet.payload.len());
        let recovered_before = self.reassembly.fec_recovered_fragment_count();
        let partial_drops_before = self.reassembly.partial_access_unit_drop_count();
        let gap_drops_before = self.reassembly.whole_access_unit_gap_drop_count();
        let reassembly_result = self.reassembly.ingest(packet);
        let recovered_now = self
            .reassembly
            .fec_recovered_fragment_count()
            .saturating_sub(recovered_before);
        self.ingress.fec_recovered_fragments = self
            .ingress
            .fec_recovered_fragments
            .saturating_add(recovered_now);
        self.ingress.reassembly_partial_access_unit_drops = self
            .ingress
            .reassembly_partial_access_unit_drops
            .saturating_add(
                self.reassembly
                    .partial_access_unit_drop_count()
                    .saturating_sub(partial_drops_before),
            );
        self.ingress.reassembly_whole_access_unit_gap_drops = self
            .ingress
            .reassembly_whole_access_unit_gap_drops
            .saturating_add(
                self.reassembly
                    .whole_access_unit_gap_drop_count()
                    .saturating_sub(gap_drops_before),
            );
        match reassembly_result {
            Ok(Some(access_unit)) => {
                if defer_until_config {
                    if access_unit.keyframe
                        && self.waiting_for_stream_config_epoch == Some(access_unit.stream_epoch)
                    {
                        self.pending_stream_config_idr = Some(access_unit);
                    }
                } else {
                    self.queue_assembled_access_unit(access_unit)?;
                }
            }
            Ok(None) => {}
            // Reassembly owns drop/keyframe-loss accounting. Keep protocol
            // rejects out of the decoder and continue the session.
            Err(ReassemblyError::TooManyFragments)
            | Err(ReassemblyError::DuplicateFragment)
            | Err(ReassemblyError::EpochMismatch)
            | Err(ReassemblyError::InvalidFecParity) => {}
        }
        if self.reassembly.take_reference_chain_loss() && !defer_until_config {
            self.enter_decoder_recovery(RecoveryReason::ReferenceAccessUnitLost, true)?;
        }
        Ok(())
    }

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

    fn queue_assembled_access_unit(
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
        #[cfg(target_os = "windows")]
        let ring = if name == DEFAULT_SHARED_RING_NAME {
            SharedFrameRingProducer::open_or_create_file(
                picoo_frame_hub::windows_shared_ring_path(name),
                picoo_frame_hub::DEFAULT_MAX_FRAME_BYTES,
            )?
        } else {
            SharedFrameRingProducer::open_or_create(name, picoo_frame_hub::DEFAULT_MAX_FRAME_BYTES)?
        };
        #[cfg(target_os = "macos")]
        let ring = if name == DEFAULT_SHARED_RING_NAME {
            let path = picoo_frame_hub::macos_app_group_ring_path(name)?;
            SharedFrameRingProducer::open_or_create_file(
                path,
                picoo_frame_hub::DEFAULT_MAX_FRAME_BYTES,
            )?
        } else {
            SharedFrameRingProducer::open_or_create(name, picoo_frame_hub::DEFAULT_MAX_FRAME_BYTES)?
        };
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let ring = SharedFrameRingProducer::open_or_create(
            name,
            picoo_frame_hub::DEFAULT_MAX_FRAME_BYTES,
        )?;
        self.shared_ring = Some(ring);
        self.publish_waiting_placeholder()?;
        Ok(())
    }

    pub fn publish_waiting_placeholder(&mut self) -> Result<(), ReceiverError> {
        let nv12 = self.placeholder_mode.waiting_frame();
        self.publish_decoded_frame(
            FrameTimeline::default(),
            DecodedFrame {
                width: PLACEHOLDER_WIDTH,
                height: PLACEHOLDER_HEIGHT,
                stride: PLACEHOLDER_WIDTH,
                rotation: 0,
                timestamp_us: 0,
                nv12: Bytes::from(nv12),
            },
        )
    }

    /// Publish reconnect-branded placeholder (REQ-PICOO-FRAME-005).
    pub fn publish_reconnecting_placeholder(&mut self) -> Result<(), ReceiverError> {
        let nv12 = self.placeholder_mode.reconnecting_frame();
        self.publish_decoded_frame(
            FrameTimeline::default(),
            DecodedFrame {
                width: PLACEHOLDER_WIDTH,
                height: PLACEHOLDER_HEIGHT,
                stride: PLACEHOLDER_WIDTH,
                rotation: 0,
                timestamp_us: 0,
                nv12: Bytes::from(nv12),
            },
        )
    }

    /// Prefer branded waiting frame (`true`) or solid black (`false`) — PRD §16.
    /// Prefer [`set_placeholder_mode`] for Logo / Black / Bars.
    pub fn set_use_default_placeholder(&mut self, enabled: bool) {
        self.placeholder_mode = if enabled {
            PlaceholderMode::Logo
        } else {
            PlaceholderMode::Black
        };
    }

    pub fn use_default_placeholder(&self) -> bool {
        matches!(self.placeholder_mode, PlaceholderMode::Logo)
    }

    pub fn set_placeholder_mode(&mut self, mode: PlaceholderMode) {
        self.placeholder_mode = mode;
    }

    pub fn placeholder_mode(&self) -> PlaceholderMode {
        self.placeholder_mode
    }

    /// Test-only decoder injection keeps synthetic payload support outside the
    /// production platform decoder.
    #[cfg(test)]
    pub fn set_decoder_for_test(&mut self, decoder: Box<dyn AccessUnitDecoder>) {
        self.decoder_worker = DecoderWorker::with_decoder(decoder);
    }

    /// Test adapter for decoder recovery fixtures without a network timeline.
    #[cfg(test)]
    pub(crate) fn publish_access_unit(
        &mut self,
        access_unit: Bytes,
        keyframe: bool,
    ) -> Result<(), ReceiverError> {
        self.publish_timeline_access_unit(EncodedAccessUnit {
            connection_generation: self
                .transport
                .active_session()
                .map_or(0, |session| session.0),
            stream_generation: self
                .current_stream_config
                .as_ref()
                .map_or(0, |config| u64::from(config.stream_epoch)),
            frame_id: 0,
            source_pts_us: 0,
            received_at_us: self.timing_origin.elapsed().as_micros() as u64,
            kind: if keyframe {
                FrameKind::Key
            } else {
                FrameKind::ReferenceDelta
            },
            data: access_unit,
        })
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

    pub(super) fn drain_decoder_events(&mut self) -> Result<(), ReceiverError> {
        while let Some(event) = self.decoder_worker.poll_event() {
            match event {
                DecoderEvent::Started => {
                    self.ingress.decode_invocations =
                        self.ingress.decode_invocations.saturating_add(1);
                }
                DecoderEvent::Completed {
                    timeline,
                    decoder_generation,
                    decoded_at,
                    decode_time_us,
                    result,
                } => {
                    self.decoder_completions = self.decoder_completions.saturating_add(1);
                    self.jitter.observe_decode_time_us(decode_time_us);
                    let decoder_generation_current = self
                        .decoder_worker
                        .is_current_generation(decoder_generation);
                    let timeline_current = self.decoder_timeline_is_current(timeline);
                    if !decoder_generation_current || !timeline_current {
                        continue;
                    }
                    if !self.decoder_recovery.accepts(timeline.kind.is_keyframe()) {
                        continue;
                    }
                    self.handle_decoder_result(timeline, decoded_at, result)?;
                }
                DecoderEvent::ResetFailed(error) => {
                    tracing::warn!(%error, "decoder reset failed; worker rebuilt platform decoder");
                    self.last_media_error = Some(format!("decoder reset failed: {error}"));
                }
            }
        }
        Ok(())
    }

    fn decoder_timeline_is_current(&self, timeline: AccessUnitTimeline) -> bool {
        #[cfg(test)]
        if timeline.connection_generation == 0 && timeline.stream_generation == 0 {
            return true;
        }
        let connection_matches = timeline.connection_generation == 0
            || self.control_generation.map_or_else(
                || {
                    self.permit_unpaired_video
                        && self
                            .transport
                            .active_session()
                            .is_some_and(|session| session.0 == timeline.connection_generation)
                },
                |generation| generation == timeline.connection_generation,
            );
        let stream_matches = self.current_stream_config.as_ref().map_or_else(
            || self.permit_unpaired_video && self.transport.active_session().is_some(),
            |config| u64::from(config.stream_epoch) == timeline.stream_generation,
        );
        connection_matches && stream_matches
    }

    fn handle_decoder_result(
        &mut self,
        timeline: AccessUnitTimeline,
        decoded_at: Instant,
        result: Result<picoo_media_decode::DecodeOutcome, picoo_media_decode::DecodeError>,
    ) -> Result<(), ReceiverError> {
        let outcome = match result {
            Ok(decoded) => decoded,
            Err(error) => {
                self.stats_reporter.record_decoder_drop();
                self.last_media_error = Some(error.to_string());
                tracing::warn!("H.264 access unit decode failed: {error}");
                self.enter_decoder_recovery(RecoveryReason::DecoderError, true)?;
                return Ok(());
            }
        };
        if timeline.kind.is_keyframe() && outcome.refresh_accepted {
            self.mark_decoder_refresh_accepted();
        }
        match outcome.frame {
            Some(mut frame) => {
                // Prefer StreamConfig.rotation from Sender when present (PUC-005 / MEDIA-009).
                frame.rotation = self
                    .current_stream_config
                    .as_ref()
                    .map(|c| c.rotation)
                    .unwrap_or(frame.rotation);
                self.publish_decoded_frame(
                    FrameTimeline {
                        stream_generation: timeline.stream_generation,
                        frame_id: timeline.frame_id,
                        source_pts_us: timeline.source_pts_us,
                        received_at_us: timeline.received_at_us,
                        decoded_at: Some(decoded_at),
                    },
                    frame,
                )?;
                self.ingress.decoded_frames += 1;
                self.stats_reporter.record_decoded_frame();
                self.last_media_error = None;
            }
            None => {
                self.stats_reporter.record_decoder_drop();
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn drain_decoder_until_idle_for_test(&mut self) {
        let expected_completion = self.decoder_completions.saturating_add(1);
        let deadline = Instant::now() + std::time::Duration::from_secs(1);
        while Instant::now() < deadline {
            self.drain_decoder_events().expect("decoder events");
            if self.decoder_completions >= expected_completion {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("decoder worker did not complete within test deadline");
    }

    fn publish_decoded_frame(
        &mut self,
        timeline: FrameTimeline,
        frame: DecodedFrame,
    ) -> Result<(), ReceiverError> {
        let DecodedFrame {
            width,
            height,
            stride,
            rotation,
            timestamp_us,
            nv12,
        } = frame;
        let mirrored = self
            .current_stream_config
            .as_ref()
            .is_some_and(|c| c.mirrored);
        // REQ-PICOO-MEDIA-004/009/017: rotate then mirror in one output pass.
        let transformed = picoo_frame_hub::transform_nv12_with_pool(
            width,
            height,
            stride,
            rotation,
            mirrored,
            nv12,
            &self.frame_buffer_pool,
        )?;
        let (width, height, stride, pixels) = (
            transformed.width,
            transformed.height,
            transformed.stride,
            transformed.pixels,
        );

        // Pixels are upright after rotation; clear metadata so VCam does not double-rotate.
        let published_rotation = 0u32;

        let published = self.latest_frame_store.publish(VideoFrame::new(
            timeline.stream_generation,
            timeline.frame_id,
            timeline.source_pts_us,
            timeline.received_at_us,
            timeline.decoded_at.unwrap_or_else(Instant::now),
            timestamp_us,
            width,
            height,
            stride,
            published_rotation,
            pixels,
        ));
        if let Some(ring) = self.shared_ring.as_mut() {
            ring.publish_nv12(
                width,
                height,
                stride,
                published_rotation,
                timestamp_us,
                &published.pixel_data,
            )?;
        }
        Ok(())
    }

    pub fn latest_frame(&self) -> Option<&Arc<VideoFrame>> {
        self.latest_frame_store.latest()
    }
}
