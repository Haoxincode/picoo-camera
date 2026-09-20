//! Fixed source-media sampling for processed recording — REQ-PICOO-MEDIA-081.
//!
//! This component owns no GPU or codec resources. It converts ordered
//! `NativeVideoFrame` identity and PTS facts into an explicit encode/skip decision. The future
//! RenderedRecorder worker remains responsible for FrameBus polling, GPU work,
//! encoding, muxing and durable gap publication.

use picoo_frame_hub::FrameIdentity;

const SEGMENT_DURATION_SECONDS: u64 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SourceGeneration {
    connection: u64,
    stream_epoch: u64,
    decoder: u64,
    config_revision: u64,
}

impl SourceGeneration {
    fn new(identity: FrameIdentity, config_revision: u64) -> Self {
        Self {
            connection: identity.connection_generation,
            stream_epoch: identity.stream_epoch,
            decoder: identity.decoder_generation,
            config_revision,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderedSample {
    pub source: FrameIdentity,
    pub source_pts_us: u64,
    /// Timeline relative to the first encoded frame of this segment.
    pub segment_pts_us: u64,
    /// Fixed output slots skipped since the preceding encoded sample.
    pub missed_slots: u64,
    pub segment_generation: u64,
    pub force_idr: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderedSampleDecision {
    Skip,
    Encode(RenderedSample),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RenderedTimelineError {
    #[error("processed recording supports only 30fps or 60fps")]
    UnsupportedFrameRate,
    #[error("processed recording source frame identity did not advance")]
    SourceIdentityRegressed,
    #[error("processed recording source PTS did not advance")]
    SourcePtsRegressed,
    #[error("processed recording timeline generation exhausted")]
    GenerationExhausted,
    #[error("processed recording timeline arithmetic overflow")]
    TimelineOverflow,
}

/// Samples ordered source frames against absolute rational output slots.
///
/// Slot times are derived from `slot * 1_000_000 / fps`; the implementation
/// never accumulates a truncated per-frame duration. A source-generation or
/// configuration change starts a new segment at PTS zero.
pub struct RenderedTimeline {
    fps: u32,
    generation: Option<SourceGeneration>,
    segment_generation: u64,
    anchor_source_pts_us: u64,
    segment_start_slot: u64,
    next_slot: u64,
    last_source: Option<FrameIdentity>,
    last_source_pts_us: u64,
}

impl RenderedTimeline {
    pub fn new(fps: u32) -> Result<Self, RenderedTimelineError> {
        if !matches!(fps, 30 | 60) {
            return Err(RenderedTimelineError::UnsupportedFrameRate);
        }
        Ok(Self {
            fps,
            generation: None,
            segment_generation: 0,
            anchor_source_pts_us: 0,
            segment_start_slot: 0,
            next_slot: 0,
            last_source: None,
            last_source_pts_us: 0,
        })
    }

    pub fn offer(
        &mut self,
        source: FrameIdentity,
        config_revision: u64,
        source_pts_us: u64,
    ) -> Result<RenderedSampleDecision, RenderedTimelineError> {
        let generation = SourceGeneration::new(source, config_revision);
        let Some(current_generation) = self.generation else {
            let segment_generation = self
                .segment_generation
                .checked_add(1)
                .ok_or(RenderedTimelineError::GenerationExhausted)?;
            self.segment_generation = segment_generation;
            self.generation = Some(generation);
            self.anchor_source_pts_us = source_pts_us;
            self.segment_start_slot = 0;
            self.next_slot = 1;
            self.last_source = Some(source);
            self.last_source_pts_us = source_pts_us;
            return Ok(RenderedSampleDecision::Encode(RenderedSample {
                source,
                source_pts_us,
                segment_pts_us: 0,
                missed_slots: 0,
                segment_generation: self.segment_generation,
                force_idr: true,
            }));
        };

        let current_stream = (
            current_generation.connection,
            current_generation.stream_epoch,
        );
        let offered_stream = (generation.connection, generation.stream_epoch);
        if offered_stream < current_stream {
            return Err(RenderedTimelineError::SourceIdentityRegressed);
        }

        if offered_stream == current_stream {
            if generation.decoder < current_generation.decoder
                || generation.config_revision < current_generation.config_revision
            {
                return Err(RenderedTimelineError::SourceIdentityRegressed);
            }
            let previous = self.last_source.expect("active source generation");
            if source.frame_id <= previous.frame_id {
                return Err(RenderedTimelineError::SourceIdentityRegressed);
            }
            if source_pts_us <= self.last_source_pts_us {
                return Err(RenderedTimelineError::SourcePtsRegressed);
            }
        }

        if generation != current_generation {
            let segment_generation = self
                .segment_generation
                .checked_add(1)
                .ok_or(RenderedTimelineError::GenerationExhausted)?;
            self.segment_generation = segment_generation;
            self.generation = Some(generation);
            self.anchor_source_pts_us = source_pts_us;
            self.segment_start_slot = 0;
            self.next_slot = 1;
            self.last_source = Some(source);
            self.last_source_pts_us = source_pts_us;
            return Ok(RenderedSampleDecision::Encode(RenderedSample {
                source,
                source_pts_us,
                segment_pts_us: 0,
                missed_slots: 0,
                segment_generation,
                force_idr: true,
            }));
        }

        let elapsed = source_pts_us
            .checked_sub(self.anchor_source_pts_us)
            .ok_or(RenderedTimelineError::SourcePtsRegressed)?;
        let reached_slot = reached_slot(elapsed, self.fps)?;
        if reached_slot < self.next_slot {
            self.last_source = Some(source);
            self.last_source_pts_us = source_pts_us;
            return Ok(RenderedSampleDecision::Skip);
        }
        let missed_slots = reached_slot - self.next_slot;
        let scheduled_boundary = reached_slot - self.segment_start_slot
            >= u64::from(self.fps) * SEGMENT_DURATION_SECONDS;
        let force_idr = missed_slots != 0 || scheduled_boundary;
        let (segment_generation, segment_start_slot) = if force_idr {
            let segment_generation = self
                .segment_generation
                .checked_add(1)
                .ok_or(RenderedTimelineError::GenerationExhausted)?;
            (segment_generation, reached_slot)
        } else {
            (self.segment_generation, self.segment_start_slot)
        };
        let segment_pts_us = slot_pts_us(reached_slot, self.fps)?
            .checked_sub(slot_pts_us(segment_start_slot, self.fps)?)
            .ok_or(RenderedTimelineError::TimelineOverflow)?;
        let next_slot = reached_slot
            .checked_add(1)
            .ok_or(RenderedTimelineError::TimelineOverflow)?;
        self.segment_generation = segment_generation;
        self.segment_start_slot = segment_start_slot;
        self.next_slot = next_slot;
        self.last_source = Some(source);
        self.last_source_pts_us = source_pts_us;
        Ok(RenderedSampleDecision::Encode(RenderedSample {
            source,
            source_pts_us,
            segment_pts_us,
            missed_slots,
            segment_generation: self.segment_generation,
            force_idr,
        }))
    }
}

/// Returns the greatest slot whose floored microsecond timestamp is no later
/// than `elapsed_us`. This is the exact inverse boundary of `slot_pts_us`.
fn reached_slot(elapsed_us: u64, fps: u32) -> Result<u64, RenderedTimelineError> {
    let elapsed_exclusive = u128::from(elapsed_us) + 1;
    u64::try_from((elapsed_exclusive * u128::from(fps) - 1) / 1_000_000u128)
        .map_err(|_| RenderedTimelineError::TimelineOverflow)
}

fn slot_pts_us(slot: u64, fps: u32) -> Result<u64, RenderedTimelineError> {
    u64::try_from(u128::from(slot) * 1_000_000u128 / u128::from(fps))
        .map_err(|_| RenderedTimelineError::TimelineOverflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(frame_id: u64) -> FrameIdentity {
        FrameIdentity {
            connection_generation: 1,
            stream_epoch: 2,
            decoder_generation: 3,
            frame_id,
        }
    }

    fn encoded(decision: RenderedSampleDecision) -> RenderedSample {
        match decision {
            RenderedSampleDecision::Encode(sample) => sample,
            RenderedSampleDecision::Skip => panic!("expected encoded sample"),
        }
    }

    #[test]
    fn sixty_fps_source_is_sampled_at_fixed_thirty_fps_slots() {
        let mut timeline = RenderedTimeline::new(30).unwrap();
        assert_eq!(
            encoded(timeline.offer(identity(1), 4, 0).unwrap()).segment_pts_us,
            0
        );
        assert_eq!(
            timeline.offer(identity(2), 4, 16_666).unwrap(),
            RenderedSampleDecision::Skip
        );
        let second = encoded(timeline.offer(identity(3), 4, 33_333).unwrap());
        assert_eq!(second.segment_pts_us, 33_333);
        assert_eq!(second.missed_slots, 0);
        assert_eq!(
            timeline.offer(identity(4), 4, 50_000).unwrap(),
            RenderedSampleDecision::Skip
        );
        let third = encoded(timeline.offer(identity(5), 4, 66_666).unwrap());
        assert_eq!(third.segment_pts_us, 66_666);
        assert_eq!(third.missed_slots, 0);
    }

    #[test]
    fn missing_source_time_reports_skipped_output_slots() {
        let mut timeline = RenderedTimeline::new(30).unwrap();
        encoded(timeline.offer(identity(1), 4, 10_000).unwrap());
        let resumed = encoded(timeline.offer(identity(2), 4, 110_000).unwrap());
        assert_eq!(resumed.segment_generation, 2);
        assert_eq!(resumed.segment_pts_us, 0);
        assert_eq!(resumed.missed_slots, 2);
        assert!(resumed.force_idr);
        let continued = encoded(timeline.offer(identity(3), 4, 143_334).unwrap());
        assert_eq!(continued.segment_generation, 2);
        assert_eq!(continued.segment_pts_us, 33_333);
        assert_eq!(continued.missed_slots, 0);
        assert!(!continued.force_idr);
    }

    #[test]
    fn ten_second_boundary_starts_a_new_idr_segment_without_reporting_a_gap() {
        let mut timeline = RenderedTimeline::new(30).unwrap();
        for slot in 0..300 {
            let sample = encoded(
                timeline
                    .offer(identity(slot + 1), 4, slot_pts_us(slot, 30).unwrap())
                    .unwrap(),
            );
            assert_eq!(sample.segment_generation, 1);
            assert!(!sample.force_idr || slot == 0);
        }
        let boundary = encoded(timeline.offer(identity(301), 4, 10_000_000).unwrap());
        assert_eq!(boundary.segment_generation, 2);
        assert_eq!(boundary.segment_pts_us, 0);
        assert_eq!(boundary.missed_slots, 0);
        assert!(boundary.force_idr);
    }

    #[test]
    fn source_or_configuration_generation_starts_an_idr_segment() {
        let mut timeline = RenderedTimeline::new(60).unwrap();
        let first = encoded(timeline.offer(identity(8), 4, 900_000).unwrap());
        assert_eq!(first.segment_generation, 1);
        assert!(first.force_idr);

        let changed = encoded(timeline.offer(identity(9), 5, 916_666).unwrap());
        assert_eq!(changed.segment_generation, 2);
        assert_eq!(changed.segment_pts_us, 0);
        assert!(changed.force_idr);

        let mut next_identity = identity(1);
        next_identity.stream_epoch = 9;
        let next = encoded(timeline.offer(next_identity, 5, 0).unwrap());
        assert_eq!(next.segment_generation, 3);
        assert!(next.force_idr);
    }

    #[test]
    fn old_stream_and_same_stream_generation_regressions_are_rejected() {
        let mut timeline = RenderedTimeline::new(30).unwrap();
        encoded(timeline.offer(identity(8), 4, 900_000).unwrap());

        let mut decoder_advanced = identity(9);
        decoder_advanced.decoder_generation = 4;
        encoded(timeline.offer(decoder_advanced, 5, 933_333).unwrap());

        let mut old_decoder = identity(10);
        old_decoder.decoder_generation = 3;
        assert_eq!(
            timeline.offer(old_decoder, 5, 966_666),
            Err(RenderedTimelineError::SourceIdentityRegressed)
        );
        let mut old_config = identity(10);
        old_config.decoder_generation = 4;
        assert_eq!(
            timeline.offer(old_config, 4, 966_666),
            Err(RenderedTimelineError::SourceIdentityRegressed)
        );
        old_config.frame_id = 9;
        assert_eq!(
            timeline.offer(old_config, 6, 966_666),
            Err(RenderedTimelineError::SourceIdentityRegressed)
        );
        old_config.frame_id = 10;
        assert_eq!(
            timeline.offer(old_config, 6, 900_000),
            Err(RenderedTimelineError::SourcePtsRegressed)
        );
        encoded(timeline.offer(old_config, 6, 966_666).unwrap());

        let mut next_stream = identity(1);
        next_stream.stream_epoch = 3;
        next_stream.decoder_generation = 0;
        encoded(timeline.offer(next_stream, 0, 0).unwrap());
        let mut old_stream = identity(11);
        old_stream.stream_epoch = 2;
        assert_eq!(
            timeline.offer(old_stream, 6, 1_000_000),
            Err(RenderedTimelineError::SourceIdentityRegressed)
        );

        let mut next_connection = identity(1);
        next_connection.connection_generation = 2;
        next_connection.stream_epoch = 0;
        next_connection.decoder_generation = 0;
        encoded(timeline.offer(next_connection, 0, 0).unwrap());
        let mut old_connection = identity(12);
        old_connection.stream_epoch = u64::MAX;
        assert_eq!(
            timeline.offer(old_connection, 7, 1_100_000),
            Err(RenderedTimelineError::SourceIdentityRegressed)
        );
    }

    #[test]
    fn same_generation_requires_strict_identity_and_pts_order() {
        let mut timeline = RenderedTimeline::new(30).unwrap();
        encoded(timeline.offer(identity(1), 4, 10).unwrap());
        assert_eq!(
            timeline.offer(identity(1), 4, 20),
            Err(RenderedTimelineError::SourceIdentityRegressed)
        );
        assert_eq!(
            timeline.offer(identity(2), 4, 10),
            Err(RenderedTimelineError::SourcePtsRegressed)
        );
    }

    #[test]
    fn only_product_frame_rates_are_accepted() {
        assert!(matches!(
            RenderedTimeline::new(24),
            Err(RenderedTimelineError::UnsupportedFrameRate)
        ));
        assert!(RenderedTimeline::new(30).is_ok());
        assert!(RenderedTimeline::new(60).is_ok());
    }

    #[test]
    fn arithmetic_and_generation_boundaries_fail_without_consuming_source() {
        for fps in [30, 60] {
            for slot in 0..120 {
                let boundary = slot_pts_us(slot, fps).unwrap();
                assert_eq!(reached_slot(boundary, fps).unwrap(), slot);
                if slot != 0 && boundary != 0 {
                    assert!(reached_slot(boundary - 1, fps).unwrap() < slot);
                }
            }
        }
        assert_eq!(
            slot_pts_us(u64::MAX, 30),
            Err(RenderedTimelineError::TimelineOverflow)
        );

        let mut timeline = RenderedTimeline::new(30).unwrap();
        encoded(timeline.offer(identity(1), 4, 0).unwrap());
        timeline.segment_generation = u64::MAX;
        assert_eq!(
            timeline.offer(identity(2), 4, 100_000),
            Err(RenderedTimelineError::GenerationExhausted)
        );
        timeline.segment_generation = 1;
        let resumed = encoded(timeline.offer(identity(2), 4, 33_333).unwrap());
        assert_eq!(resumed.source.frame_id, 2);
        assert_eq!(resumed.segment_pts_us, 33_333);
    }
}
