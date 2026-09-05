//! Decoder completion handling and immutable frame publication.
//!
//! REQ-PICOO-FRAME-008/009, REQ-PICOO-MEDIA-004/009/017/023.

use std::sync::Arc;
use std::time::Instant;

use picoo_frame_hub::VideoFrame;
use picoo_media_decode::DecodedFrame;

use super::decoder_worker::{AccessUnitTimeline, DecoderEvent};
use super::recovery::RecoveryReason;
use super::ReceiverSession;
use crate::ReceiverError;

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct FrameTimeline {
    pub(super) stream_generation: u64,
    pub(super) frame_id: u64,
    pub(super) source_pts_us: u64,
    pub(super) encoded_at_us: u64,
    pub(super) received_at_us: u64,
    pub(super) decode_submitted_at_us: u64,
    pub(super) decoded_at: Option<Instant>,
}

impl ReceiverSession {
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

    pub(super) fn decoder_timeline_is_current(&self, timeline: AccessUnitTimeline) -> bool {
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
                let rotation = self
                    .current_stream_config
                    .as_ref()
                    .map(|config| config.rotation)
                    .unwrap_or(frame.description().rotation);
                frame.set_rotation(rotation);
                self.publish_decoded_frame(
                    FrameTimeline {
                        stream_generation: timeline.stream_generation,
                        frame_id: timeline.frame_id,
                        source_pts_us: timeline.source_pts_us,
                        encoded_at_us: timeline.encoded_at_us,
                        received_at_us: timeline.received_at_us,
                        decode_submitted_at_us: timeline.decode_submitted_at_us,
                        decoded_at: Some(decoded_at),
                    },
                    frame,
                )?;
                self.ingress.decoded_frames += 1;
                self.stats_reporter.record_decoded_frame();
                self.last_media_error = None;
            }
            None => self.stats_reporter.record_decoder_drop(),
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

    pub(super) fn publish_decoded_frame(
        &mut self,
        timeline: FrameTimeline,
        frame: DecodedFrame,
    ) -> Result<(), ReceiverError> {
        let description = frame.description();
        let timestamp_us = frame.timestamp_us();
        let nv12 = frame.into_cpu_nv12();
        let (width, height, stride, rotation) = (
            description.width,
            description.height,
            description.stride,
            description.rotation,
        );
        let mirrored = self
            .current_stream_config
            .as_ref()
            .is_some_and(|config| config.mirrored);
        let transform_required =
            picoo_frame_hub::normalize_rotation_degrees(rotation) != 0 || mirrored;
        let transform_started = Instant::now();
        let transformed = picoo_frame_hub::transform_nv12_with_pool(
            width,
            height,
            stride,
            rotation,
            mirrored,
            nv12,
            &self.frame_buffer_pool,
        )?;
        if transform_required {
            let elapsed_us = transform_started
                .elapsed()
                .as_micros()
                .min(u128::from(u64::MAX)) as u64;
            self.ingress.orientation_transform_frames =
                self.ingress.orientation_transform_frames.saturating_add(1);
            self.ingress.orientation_transform_total_us = self
                .ingress
                .orientation_transform_total_us
                .saturating_add(elapsed_us);
            self.ingress.orientation_transform_max_us =
                self.ingress.orientation_transform_max_us.max(elapsed_us);
        }

        let published = self.latest_frame_store.publish(VideoFrame::new(
            timeline.stream_generation,
            timeline.frame_id,
            timeline.source_pts_us,
            timeline.encoded_at_us,
            timeline.received_at_us,
            timeline.decode_submitted_at_us,
            timeline.decoded_at.unwrap_or_else(Instant::now),
            timestamp_us,
            transformed.width,
            transformed.height,
            transformed.stride,
            0,
            transformed.pixels,
        ));
        if let Some(ring) = self.shared_ring.as_ref() {
            if ring.submit(published) == picoo_frame_hub::SharedRingSubmitOutcome::Stopped {
                self.last_shared_ring_error = Some("Shared Frame Ring writer stopped".into());
            }
        }
        Ok(())
    }

    pub fn latest_frame(&self) -> Option<&Arc<VideoFrame>> {
        self.latest_frame_store.latest()
    }
}
