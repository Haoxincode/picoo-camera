//! Decoder completion handling and immutable frame publication.
//!
//! REQ-PICOO-FRAME-008/009, REQ-PICOO-MEDIA-004/009/017/023.

use std::sync::Arc;
use std::time::Instant;

#[cfg(not(any(target_os = "macos", windows)))]
use picoo_frame_hub::VideoFrame;
#[cfg(not(any(target_os = "macos", windows)))]
use picoo_media_decode::DecodedFrame;

use super::decoder_worker::{AccessUnitTimeline, DecoderEvent};
use super::recovery::RecoveryReason;
use super::ReceiverSession;
use crate::ReceiverError;

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct FrameTimeline {
    #[cfg(any(target_os = "macos", windows))]
    pub(super) connection_generation: u64,
    #[cfg(any(target_os = "macos", windows))]
    pub(super) decoder_generation: u64,
    #[cfg(any(target_os = "macos", windows))]
    pub(super) config_revision: u64,
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
                DecoderEvent::Capabilities(result) => {
                    self.receiver_capabilities_sent = None;
                    self.decoder_readiness = match result {
                        Ok(caps) => super::decoder_capabilities::DecoderReadiness::Ready(caps),
                        Err(error) => {
                            super::decoder_capabilities::DecoderReadiness::Unavailable(error)
                        }
                    };
                }
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
                    if !self.decoder_recovery.accepts_completion(timeline) {
                        continue;
                    }
                    self.handle_decoder_result(timeline, decoded_at, result)?;
                }
                DecoderEvent::Unavailable(error) => {
                    tracing::warn!(%error, "decoder failed; worker stopped and capability evidence invalidated");
                    self.last_media_error = Some(format!("native decoder failed: {error}"));
                    self.decoder_readiness =
                        super::decoder_capabilities::DecoderReadiness::Unavailable(error);
                    self.receiver_capabilities_sent = None;
                }
            }
        }
        self.finish_decoder_negotiation()
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
        if timeline.kind.is_keyframe() {
            if outcome.refresh_accepted {
                self.mark_decoder_refresh_accepted(timeline);
            } else if self.decoder_recovery.is_refresh_candidate(timeline) {
                self.enter_decoder_recovery(RecoveryReason::DecoderError, true)?;
                return Ok(());
            }
        }
        for output in outcome.frames {
            let token = output.token;
            let timeline = token.timeline;
            if !self
                .decoder_worker
                .is_current_generation(token.decoder_generation)
                || !self.decoder_timeline_is_current(timeline)
                || !self.decoder_recovery.accepts_completion(timeline)
            {
                continue;
            }
            let stream_config = token.stream_config.as_deref();
            let mut frame = output.frame;
            // REQ-PICOO-MEDIA-025: presentation belongs to the submitted AU.
            let rotation = stream_config
                .map(|config| config.rotation)
                .unwrap_or(frame.description().rotation);
            frame.set_rotation(rotation);
            self.publish_decoded_frame(
                FrameTimeline {
                    #[cfg(any(target_os = "macos", windows))]
                    connection_generation: timeline.connection_generation,
                    #[cfg(any(target_os = "macos", windows))]
                    decoder_generation: token.decoder_generation,
                    #[cfg(any(target_os = "macos", windows))]
                    config_revision: token.config_revision,
                    stream_generation: timeline.stream_generation,
                    frame_id: timeline.frame_id,
                    source_pts_us: timeline.source_pts_us,
                    encoded_at_us: timeline.encoded_at_us,
                    received_at_us: timeline.received_at_us,
                    decode_submitted_at_us: timeline.decode_submitted_at_us,
                    decoded_at: Some(decoded_at),
                },
                frame,
                stream_config.is_some_and(|config| config.mirrored),
            )?;
            self.ingress.decoded_frames += 1;
            self.stats_reporter.record_decoded_frame();
            self.last_media_error = None;
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

    #[cfg(not(any(target_os = "macos", windows)))]
    pub(super) fn publish_decoded_frame(
        &mut self,
        timeline: FrameTimeline,
        frame: DecodedFrame,
        mirrored: bool,
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

        let published = self.frames.publish(VideoFrame::new(
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

    pub fn latest_frame(&self) -> Option<&Arc<crate::ReceiverFrame>> {
        self.frames.latest()
    }
}

#[cfg(test)]
mod tests;
