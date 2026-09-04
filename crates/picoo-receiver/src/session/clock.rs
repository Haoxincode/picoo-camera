//! Receiver-initiated PCP monotonic clock synchronization.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use picoo_protocol::control::{
    control_envelope::Payload as ControlPayload, ClockSyncPing, ClockSyncPong,
};
use picoo_session::{AffineClockMapper, ClockMappingEstimate, ClockSyncExchange};

use super::ReceiverSession;
use crate::ReceiverError;

const INITIAL_SYNC_INTERVAL: Duration = Duration::from_millis(250);
const STEADY_SYNC_INTERVAL: Duration = Duration::from_secs(1);
const MAX_PENDING_SYNC_SAMPLES: usize = 4;

#[derive(Debug, Clone, Copy)]
struct PendingClockSync {
    sample_id: u64,
    receiver_send_us: u64,
    stream_epoch: u32,
}

#[derive(Debug)]
pub(super) struct ReceiverClockSync {
    mapper: AffineClockMapper,
    pending: VecDeque<PendingClockSync>,
    next_sample_id: u64,
    next_sync_at: Option<Instant>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct FrameLatencyBreakdown {
    pub(super) capture_to_encode_ms: Option<f64>,
    pub(super) encode_to_arrival_ms: Option<f64>,
    pub(super) jitter_residence_ms: Option<f64>,
    pub(super) decode_ms: Option<f64>,
    pub(super) frame_publish_age_ms: Option<f64>,
    pub(super) end_to_end_latency_ms: Option<f64>,
    pub(super) clock_uncertainty_ms: Option<f64>,
}

impl Default for ReceiverClockSync {
    fn default() -> Self {
        Self {
            mapper: AffineClockMapper::new(0),
            pending: VecDeque::with_capacity(MAX_PENDING_SYNC_SAMPLES),
            next_sample_id: 1,
            next_sync_at: None,
        }
    }
}

impl ReceiverClockSync {
    fn reset(&mut self, stream_epoch: u32) {
        self.mapper.reset(u64::from(stream_epoch));
        self.pending.clear();
        self.next_sample_id = 1;
        self.next_sync_at = Some(Instant::now());
    }

    fn clear(&mut self) {
        self.mapper.reset(0);
        self.pending.clear();
        self.next_sample_id = 1;
        self.next_sync_at = None;
    }

    fn due(&self, now: Instant) -> bool {
        self.next_sync_at.is_some_and(|deadline| now >= deadline)
    }

    fn create_ping(&mut self, now: Instant, receiver_send_us: u64) -> Option<ClockSyncPing> {
        if !self.due(now) || self.mapper.generation() == 0 {
            return None;
        }
        let sample_id = self.next_sample_id;
        self.next_sample_id = self.next_sample_id.checked_add(1)?;
        let stream_epoch = u32::try_from(self.mapper.generation()).ok()?;
        // A lost reliable-control response must not permanently stop clock
        // synchronization. Keep only the newest bounded request window.
        if self.pending.len() >= MAX_PENDING_SYNC_SAMPLES {
            self.pending.pop_front();
        }
        self.pending.push_back(PendingClockSync {
            sample_id,
            receiver_send_us,
            stream_epoch,
        });
        self.next_sync_at = Some(
            now + if self.mapper.is_stable() {
                STEADY_SYNC_INTERVAL
            } else {
                INITIAL_SYNC_INTERVAL
            },
        );
        Some(ClockSyncPing {
            sample_id,
            receiver_send_us,
            stream_epoch,
        })
    }

    fn observe_pong(&mut self, pong: &ClockSyncPong, receiver_receive_us: u64) {
        let Some(index) = self.pending.iter().position(|pending| {
            pending.sample_id == pong.sample_id
                && pending.receiver_send_us == pong.receiver_send_us
                && pending.stream_epoch == pong.stream_epoch
        }) else {
            return;
        };
        let pending = self.pending.remove(index).expect("pending index exists");
        let _ = self.mapper.observe(ClockSyncExchange {
            generation: u64::from(pending.stream_epoch),
            local_send_us: pending.receiver_send_us,
            remote_receive_us: pong.sender_receive_us,
            remote_send_us: pong.sender_send_us,
            local_receive_us: receiver_receive_us,
        });
    }

    fn estimate_local_time(&self, sender_time_us: u64) -> Option<ClockMappingEstimate> {
        self.mapper.estimate_local_time(sender_time_us)
    }
}

impl ReceiverSession {
    pub(super) fn reset_clock_sync(&mut self, stream_epoch: u32) {
        self.clock_sync.reset(stream_epoch);
    }

    pub(super) fn clear_clock_sync(&mut self) {
        self.clock_sync.clear();
    }

    pub(super) fn handle_clock_sync_pong(&mut self, pong: ClockSyncPong) {
        let receiver_receive_us = self.timing_origin.elapsed().as_micros() as u64;
        self.clock_sync.observe_pong(&pong, receiver_receive_us);
    }

    pub(super) fn mapped_sender_time(
        &self,
        stream_epoch: u64,
        sender_time_us: u64,
    ) -> Option<ClockMappingEstimate> {
        (self.clock_sync.mapper.generation() == stream_epoch)
            .then(|| self.clock_sync.estimate_local_time(sender_time_us))
            .flatten()
    }

    pub(super) fn frame_latency_breakdown(
        &self,
        frame: &picoo_frame_hub::VideoFrame,
        receiver_now_us: u64,
    ) -> FrameLatencyBreakdown {
        let decoded_at_us = frame
            .decoded_at
            .saturating_duration_since(self.timing_origin)
            .as_micros() as u64;
        let capture_to_encode_ms = frame
            .encoded_at_us
            .checked_sub(frame.source_pts_us)
            .map(microseconds_to_milliseconds);
        let jitter_residence_ms = frame
            .decode_submitted_at_us
            .checked_sub(frame.received_at_us)
            .map(microseconds_to_milliseconds);
        let decode_ms = decoded_at_us
            .checked_sub(frame.decode_submitted_at_us)
            .map(microseconds_to_milliseconds);
        let frame_publish_age_ms = receiver_now_us
            .checked_sub(decoded_at_us)
            .map(microseconds_to_milliseconds);

        let mapped_capture = self.mapped_sender_time(frame.stream_generation, frame.source_pts_us);
        let mapped_encoded = self.mapped_sender_time(frame.stream_generation, frame.encoded_at_us);
        FrameLatencyBreakdown {
            capture_to_encode_ms,
            encode_to_arrival_ms: mapped_encoded.and_then(|mapped| {
                frame
                    .received_at_us
                    .checked_sub(mapped.local_time_us)
                    .map(microseconds_to_milliseconds)
            }),
            jitter_residence_ms,
            decode_ms,
            frame_publish_age_ms,
            end_to_end_latency_ms: mapped_capture.and_then(|mapped| {
                receiver_now_us
                    .checked_sub(mapped.local_time_us)
                    .map(microseconds_to_milliseconds)
            }),
            clock_uncertainty_ms: mapped_capture
                .map(|mapped| microseconds_to_milliseconds(mapped.uncertainty_us)),
        }
    }

    pub(super) fn maybe_send_clock_sync(&mut self) -> Result<(), ReceiverError> {
        if !self.video_allowed() || !self.lifecycle.runtime.stream().is_streaming() {
            return Ok(());
        }
        let Some(session) = self.transport.active_session() else {
            return Ok(());
        };
        let now = Instant::now();
        let receiver_send_us = self.timing_origin.elapsed().as_micros() as u64;
        let Some(ping) = self.clock_sync.create_ping(now, receiver_send_us) else {
            return Ok(());
        };
        self.send_control_payload(session, ControlPayload::ClockSyncPing(ping))
    }
}

fn microseconds_to_milliseconds(value: u64) -> f64 {
    value as f64 / 1_000.0
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use picoo_frame_hub::VideoFrame;

    use super::*;

    fn observe_exchange(
        sync: &mut ReceiverClockSync,
        now: Instant,
        sample_index: u64,
        stream_epoch: u32,
    ) -> ClockSyncPong {
        let local_midpoint_us = 1_000_000 + sample_index * 300_000;
        let ping = sync
            .create_ping(
                now + Duration::from_millis(sample_index * 300),
                local_midpoint_us - 2_000,
            )
            .expect("sync ping");
        let remote_midpoint_us = local_midpoint_us - 100_000;
        let pong = ClockSyncPong {
            sample_id: ping.sample_id,
            receiver_send_us: ping.receiver_send_us,
            sender_receive_us: remote_midpoint_us,
            sender_send_us: remote_midpoint_us,
            stream_epoch,
        };
        sync.observe_pong(&pong, local_midpoint_us + 2_000);
        pong
    }

    #[test]
    fn mapping_and_total_latency_remain_hidden_until_three_spanning_pongs() {
        let mut session = ReceiverSession::new();
        session.reset_clock_sync(7);
        let now = Instant::now();

        observe_exchange(&mut session.clock_sync, now, 0, 7);
        observe_exchange(&mut session.clock_sync, now, 1, 7);
        assert!(session.mapped_sender_time(7, 1_700_000).is_none());

        observe_exchange(&mut session.clock_sync, now, 2, 7);
        let mapped = session
            .mapped_sender_time(7, 1_700_000)
            .expect("stable affine mapping");
        assert!(mapped.local_time_us.abs_diff(1_800_000) <= 2);

        let frame = VideoFrame::new(
            7,
            1,
            1_700_000,
            1_705_000,
            1_810_000,
            1_820_000,
            session.timing_origin + Duration::from_micros(1_850_000),
            0,
            2,
            2,
            2,
            0,
            Bytes::from_static(&[0; 6]),
        );
        let latency = session.frame_latency_breakdown(&frame, 2_000_000);
        assert_eq!(latency.capture_to_encode_ms, Some(5.0));
        assert!(latency
            .encode_to_arrival_ms
            .is_some_and(|value| (value - 5.0).abs() < 0.01));
        assert_eq!(latency.jitter_residence_ms, Some(10.0));
        assert_eq!(latency.decode_ms, Some(30.0));
        assert_eq!(latency.frame_publish_age_ms, Some(150.0));
        assert!(latency
            .end_to_end_latency_ms
            .is_some_and(|value| (value - 200.0).abs() < 0.01));
        assert_eq!(latency.clock_uncertainty_ms, Some(2.0));
    }

    #[test]
    fn epoch_reset_and_replayed_or_stale_pongs_cannot_mutate_mapping() {
        let mut sync = ReceiverClockSync::default();
        sync.reset(3);
        let now = Instant::now();
        let replay = observe_exchange(&mut sync, now, 0, 3);
        assert_eq!(sync.mapper.sample_count(), 1);
        sync.observe_pong(&replay, 1_004_000);
        assert_eq!(sync.mapper.sample_count(), 1, "replay is no longer pending");

        sync.reset(4);
        sync.observe_pong(&replay, 1_004_000);
        assert_eq!(sync.mapper.sample_count(), 0, "old epoch pong is ignored");
        assert!(!sync.mapper.is_stable());
    }

    #[test]
    fn unanswered_pings_roll_the_bounded_window_instead_of_stalling() {
        let mut sync = ReceiverClockSync::default();
        sync.reset(5);
        let now = Instant::now();
        for index in 0..=MAX_PENDING_SYNC_SAMPLES {
            assert!(sync
                .create_ping(
                    now + Duration::from_millis(index as u64 * 300),
                    1_000_000 + index as u64 * 300_000,
                )
                .is_some());
        }
        assert_eq!(sync.pending.len(), MAX_PENDING_SYNC_SAMPLES);
        assert_eq!(sync.pending.front().map(|sample| sample.sample_id), Some(2));
    }

    #[test]
    fn invalid_sender_timeline_does_not_publish_negative_segments() {
        let session = ReceiverSession::new();
        let frame = VideoFrame::new(
            1,
            1,
            20,
            10,
            100,
            90,
            session.timing_origin,
            0,
            2,
            2,
            2,
            0,
            Bytes::from_static(&[0; 6]),
        );
        let latency = session.frame_latency_breakdown(&frame, 100);
        assert_eq!(latency.capture_to_encode_ms, None);
        assert_eq!(latency.jitter_residence_ms, None);
        assert_eq!(latency.end_to_end_latency_ms, None);
    }
}
