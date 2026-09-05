//! Receiver-local media timing and statistics helpers.

use std::time::{Duration, Instant};

const MEDIA_DEADLINE_MIN_MS: f64 = 200.0;
const MEDIA_DEADLINE_MAX_MS: f64 = 300.0;

pub(super) fn media_deadline_from_observations(
    rtt_ms: f64,
    jitter_ms: f64,
    frame_ms: f64,
    playout_target_ms: f64,
) -> Duration {
    let playout_bound_ms = 2.0 * playout_target_ms + frame_ms;
    let network_bound_ms = rtt_ms + 3.0 * jitter_ms + frame_ms;
    let deadline_ms = playout_bound_ms
        .max(network_bound_ms)
        .clamp(MEDIA_DEADLINE_MIN_MS, MEDIA_DEADLINE_MAX_MS);
    Duration::from_secs_f64(deadline_ms / 1_000.0)
}

pub(super) struct StatsReporter {
    pub(super) last_sent: Instant,
    pub(super) window_bytes: u64,
    pub(super) last_reassembly_drops: u64,
    pub(super) last_missing_fragments: u64,
    pub(super) last_resolved_fragments: u64,
    pub(super) last_fec_recovered_fragments: u64,
    pub(super) window_decoder_drops: u64,
    pub(super) window_decoded_frames: u64,
    pub(super) window_max_receive_queue_age_ms: f64,
}

impl StatsReporter {
    pub(super) fn new() -> Self {
        Self {
            last_sent: Instant::now(),
            window_bytes: 0,
            last_reassembly_drops: 0,
            last_missing_fragments: 0,
            last_resolved_fragments: 0,
            last_fec_recovered_fragments: 0,
            window_decoder_drops: 0,
            window_decoded_frames: 0,
            window_max_receive_queue_age_ms: 0.0,
        }
    }

    pub(super) fn record_packet(&mut self, payload_len: usize) {
        self.window_bytes += payload_len as u64;
    }

    pub(super) fn record_decoder_drop(&mut self) {
        self.window_decoder_drops += 1;
    }

    pub(super) fn record_decoded_frame(&mut self) {
        self.window_decoded_frames += 1;
    }

    pub(super) fn record_receive_queue_age(&mut self, age: Duration) {
        self.window_max_receive_queue_age_ms = self
            .window_max_receive_queue_age_ms
            .max(age.as_secs_f64() * 1_000.0);
    }

    pub(super) fn due(&self) -> bool {
        self.last_sent.elapsed() >= Duration::from_secs(1)
    }
}

/// RFC 3550-style inter-arrival jitter estimate without synchronized clocks.
#[derive(Default)]
pub(super) struct InterarrivalJitter {
    last: Option<(Instant, u64)>,
    estimate_us: f64,
}

impl InterarrivalJitter {
    pub(super) fn observe(&mut self, arrived_at: Instant, pts_us: u64) {
        let Some((last_arrival, last_pts_us)) = self.last else {
            self.last = Some((arrived_at, pts_us));
            return;
        };
        if pts_us <= last_pts_us {
            return;
        }
        let arrival_delta_us = arrived_at.duration_since(last_arrival).as_micros() as f64;
        let pts_delta_us = pts_us.saturating_sub(last_pts_us) as f64;
        let variation_us = (arrival_delta_us - pts_delta_us).abs();
        self.estimate_us += (variation_us - self.estimate_us) / 16.0;
        self.last = Some((arrived_at, pts_us));
    }

    pub(super) fn milliseconds(&self) -> f64 {
        self.estimate_us / 1_000.0
    }

    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }
}

pub(super) fn observed_fragment_loss_ratio(resolved_fragments: u64, missing_fragments: u64) -> f64 {
    if resolved_fragments == 0 {
        0.0
    } else {
        missing_fragments.min(resolved_fragments) as f64 / resolved_fragments as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_arrivals_have_zero_jitter_and_variation_uses_ewma() {
        let start = Instant::now();
        let mut jitter = InterarrivalJitter::default();
        jitter.observe(start, 1_000_000);
        jitter.observe(start + Duration::from_millis(33), 1_033_000);
        assert_eq!(jitter.milliseconds(), 0.0);

        jitter.observe(start + Duration::from_millis(86), 1_066_000);
        assert!((jitter.milliseconds() - 1.25).abs() < 0.001);
    }

    #[test]
    fn late_older_access_unit_does_not_corrupt_estimate() {
        let start = Instant::now();
        let mut jitter = InterarrivalJitter::default();
        jitter.observe(start, 100_000);
        jitter.observe(start + Duration::from_millis(40), 90_000);
        assert_eq!(jitter.milliseconds(), 0.0);
        jitter.observe(start + Duration::from_millis(33), 133_000);
        assert_eq!(jitter.milliseconds(), 0.0);
    }

    #[test]
    fn fragment_loss_compares_received_and_missing_fragments_in_the_same_unit() {
        assert_eq!(observed_fragment_loss_ratio(0, 0), 0.0);
        assert_eq!(observed_fragment_loss_ratio(10, 1), 0.1);
        assert_eq!(observed_fragment_loss_ratio(1, 1), 1.0);
    }

    #[test]
    fn media_failure_deadline_stays_beyond_playout_and_is_hard_bounded() {
        assert_eq!(
            media_deadline_from_observations(20.0, 2.0, 33.0, 33.0),
            Duration::from_millis(200),
        );
        assert_eq!(
            media_deadline_from_observations(40.0, 20.0, 33.0, 80.0),
            Duration::from_millis(200),
        );
        assert_eq!(
            media_deadline_from_observations(150.0, 80.0, 33.0, 80.0),
            Duration::from_millis(300),
        );
    }
}
