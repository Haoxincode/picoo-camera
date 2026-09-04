//! Deadline-driven adaptive jitter buffer — REQ-PICOO-SESSION-002.
//!
//! Sender PTS and Receiver monotonic time do not share an epoch. The buffer
//! therefore estimates their relative offset from the minimum observed transit
//! value. Only relative delay variation is used; synchronized wall clocks are
//! not required.

use bytes::Bytes;
use std::collections::VecDeque;

const STARTUP_TARGET_US: u64 = 33_000;
const MIN_TARGET_US: u64 = 16_000;
const MAX_TARGET_US: u64 = 80_000;
const MAX_OBSERVED_DECODE_US: u64 = 250_000;
const DEFAULT_DECODE_US: u64 = 5_000;
const RENDER_MARGIN_US: u64 = 5_000;
const SAMPLE_WINDOW: usize = 150;
// V1 is capped at 30 FPS. Sixteen complete AUs cover more than the maximum
// 300ms reassembly failure deadline + 80ms playout target + one frame. The
// wall-clock expiry remains the latency guard; this count bound only protects
// memory and must not fire first during a legitimate reordering window.
const MAX_BUFFERED_FRAMES: usize = 16;

#[derive(Debug, Clone)]
pub struct Frame {
    pub stream_generation: u64,
    pub frame_id: u64,
    pub pts_us: u64,
    /// Completion time on the Receiver-local monotonic timeline.
    pub received_at_us: u64,
    pub data: Bytes,
    pub keyframe: bool,
    pub discardable: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TimingStats {
    /// Current total playout budget, including decode/render allowance.
    pub target_delay_ms: f64,
    /// Mean first-fragment-to-buffer-exit delay for frames emitted this window.
    pub actual_delay_ms: f64,
    /// Current newest-minus-oldest buffered PTS span.
    pub occupancy_ms: f64,
}

#[derive(Debug)]
struct BufferedFrame {
    frame: Frame,
    first_fragment_at_us: u64,
    completed_at_us: u64,
}

#[derive(Debug)]
pub struct JitterBuffer {
    frames: VecDeque<BufferedFrame>,
    last_emitted_pts_us: Option<u64>,
    base_transit_us: Option<i128>,
    last_observed_pts_us: Option<u64>,
    /// Receiver-completion minus Sender-PTS samples. Their absolute values
    /// include the unknown clock offset; distance from the rolling minimum is
    /// the queueing/arrival delay that the playout target must absorb.
    transit_samples_us: VecDeque<i128>,
    decode_times_us: VecDeque<u64>,
    adaptive_target_us: u64,
    fixed_target_us: Option<u64>,
    emitted_delay_sum_us: u128,
    emitted_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushOutcome {
    Accepted,
    AcceptedAfterReferenceDrop,
    DroppedLate { requires_refresh: bool },
}

impl Default for JitterBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl JitterBuffer {
    pub fn new() -> Self {
        Self {
            frames: VecDeque::new(),
            last_emitted_pts_us: None,
            base_transit_us: None,
            last_observed_pts_us: None,
            transit_samples_us: VecDeque::with_capacity(SAMPLE_WINDOW),
            decode_times_us: VecDeque::with_capacity(SAMPLE_WINDOW),
            adaptive_target_us: STARTUP_TARGET_US,
            fixed_target_us: None,
            emitted_delay_sum_us: 0,
            emitted_count: 0,
        }
    }

    /// Explicit fixed target for deterministic loopback/tests. Product sessions
    /// use the adaptive controller by default.
    pub fn set_fixed_target_ms(&mut self, target_ms: Option<u64>) {
        self.fixed_target_us = target_ms.map(|value| value.saturating_mul(1_000));
    }

    pub fn target_delay_ms(&self) -> f64 {
        self.target_us() as f64 / 1_000.0
    }

    pub fn observe_decode_time_us(&mut self, decode_time_us: u64) {
        push_bounded(
            &mut self.decode_times_us,
            decode_time_us.min(MAX_OBSERVED_DECODE_US),
        );
        self.update_target();
    }

    /// Queue a complete access unit. Times use a Receiver-local monotonic epoch.
    pub fn push_at(
        &mut self,
        frame: Frame,
        first_fragment_at_us: u64,
        completed_at_us: u64,
    ) -> PushOutcome {
        if self
            .last_emitted_pts_us
            .is_some_and(|emitted| frame.pts_us <= emitted)
        {
            return PushOutcome::DroppedLate {
                requires_refresh: !frame.discardable,
            };
        }

        self.observe_arrival(frame.pts_us, completed_at_us);
        let index = self
            .frames
            .iter()
            .position(|buffered| buffered.frame.pts_us > frame.pts_us)
            .unwrap_or(self.frames.len());
        self.frames.insert(
            index,
            BufferedFrame {
                frame,
                first_fragment_at_us,
                completed_at_us,
            },
        );
        if self.enforce_limits() {
            PushOutcome::AcceptedAfterReferenceDrop
        } else {
            PushOutcome::Accepted
        }
    }

    pub fn pop_ready(&mut self, now_us: u64) -> Option<Frame> {
        let front = self.frames.front()?;
        if now_us < self.decode_release_at_us(front.frame.pts_us) {
            return None;
        }
        let buffered = self.frames.pop_front()?;
        self.last_emitted_pts_us = Some(buffered.frame.pts_us);
        self.emitted_delay_sum_us = self.emitted_delay_sum_us.saturating_add(u128::from(
            now_us.saturating_sub(buffered.first_fragment_at_us),
        ));
        self.emitted_count = self.emitted_count.saturating_add(1);
        Some(buffered.frame)
    }

    /// Drop queued frames that have exceeded the absolute real-time deadline.
    /// Returns true when a non-discardable reference AU was discarded.
    pub fn drop_expired(&mut self, now_us: u64, max_queue_age_us: u64) -> bool {
        if self.fixed_target_us.is_some() {
            return false;
        }
        let max_queue_age_us = max_queue_age_us.max(self.target_us());
        let mut reference_chain_broken = false;
        self.frames.retain(|buffered| {
            // Sender PTS has no synchronized epoch and includes variable camera
            // plus encoder latency. It can order frames, but cannot prove that
            // a newly completed AU is stale. Only time spent queued *after*
            // completion is a valid local expiry signal here; reassembly owns
            // the separate first-fragment deadline.
            let deadline = buffered.completed_at_us.saturating_add(max_queue_age_us);
            let keep = now_us <= deadline;
            if !keep && !buffered.frame.discardable {
                reference_chain_broken = true;
            }
            keep
        });
        reference_chain_broken
    }

    pub fn occupancy_ms(&self) -> f64 {
        match (self.frames.front(), self.frames.back()) {
            (Some(first), Some(last)) if last.frame.pts_us >= first.frame.pts_us => {
                (last.frame.pts_us - first.frame.pts_us) as f64 / 1_000.0
            }
            _ => 0.0,
        }
    }

    pub fn take_timing_stats(&mut self) -> TimingStats {
        let actual_delay_ms = if self.emitted_count == 0 {
            0.0
        } else {
            self.emitted_delay_sum_us as f64 / self.emitted_count as f64 / 1_000.0
        };
        self.emitted_delay_sum_us = 0;
        self.emitted_count = 0;
        TimingStats {
            target_delay_ms: self.target_delay_ms(),
            actual_delay_ms,
            occupancy_ms: self.occupancy_ms(),
        }
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn front_pts_us(&self) -> Option<u64> {
        self.frames.front().map(|buffered| buffered.frame.pts_us)
    }

    pub fn front_frame_id(&self) -> Option<u64> {
        self.frames.front().map(|buffered| buffered.frame.frame_id)
    }

    /// Discard media while preserving the current network/decode estimate.
    pub fn discard_queued(&mut self) {
        self.frames.clear();
        self.last_emitted_pts_us = None;
    }

    /// Reset the entire stream timing epoch.
    pub fn clear(&mut self) {
        let fixed_target_us = self.fixed_target_us;
        *self = Self::new();
        self.fixed_target_us = fixed_target_us;
    }

    fn observe_arrival(&mut self, pts_us: u64, completed_at_us: u64) {
        let transit_us = i128::from(completed_at_us) - i128::from(pts_us);
        if self
            .last_observed_pts_us
            .is_some_and(|last_pts_us| pts_us <= last_pts_us)
        {
            // A late older AU must not move the scheduling clock backwards.
            return;
        }
        self.last_observed_pts_us = Some(pts_us);
        push_bounded(&mut self.transit_samples_us, transit_us);
        self.base_transit_us = self.transit_samples_us.iter().copied().min();
        self.update_target();
    }

    fn update_target(&mut self) {
        if self.fixed_target_us.is_some() {
            return;
        }
        // Preserve the startup budget until enough complete frames exist for
        // a meaningful arrival-variation percentile.
        if self.transit_samples_us.len() < 5 {
            return;
        }
        let base_transit_us = self.base_transit_us.unwrap_or(0);
        let mut queueing_delays_us = self
            .transit_samples_us
            .iter()
            .map(|sample| sample.saturating_sub(base_transit_us).max(0) as u64)
            .collect::<VecDeque<_>>();
        let arrival_delay_p95 = percentile(&queueing_delays_us, 95).unwrap_or(0);
        queueing_delays_us.clear();
        let decode_p95 = percentile(&self.decode_times_us, 95).unwrap_or(DEFAULT_DECODE_US);
        let desired = arrival_delay_p95
            .saturating_add(decode_p95)
            .saturating_add(RENDER_MARGIN_US)
            .clamp(MIN_TARGET_US, MAX_TARGET_US);
        if desired >= self.adaptive_target_us {
            self.adaptive_target_us = desired;
        } else {
            // At 30 FPS this drains no faster than ~30 ms/s, avoiding visible
            // jumps while still returning promptly to the LAN low-latency path.
            self.adaptive_target_us = self.adaptive_target_us.saturating_sub(1_000).max(desired);
        }
    }

    fn target_us(&self) -> u64 {
        self.fixed_target_us.unwrap_or(self.adaptive_target_us)
    }

    fn decode_budget_us(&self) -> u64 {
        percentile(&self.decode_times_us, 95)
            .unwrap_or(DEFAULT_DECODE_US)
            .saturating_add(RENDER_MARGIN_US)
    }

    fn decode_release_at_us(&self, pts_us: u64) -> u64 {
        let holdback = self.target_us().saturating_sub(self.decode_budget_us());
        mapped_time_us(self.base_transit_us, pts_us).saturating_add(holdback)
    }

    fn enforce_limits(&mut self) -> bool {
        let mut reference_chain_broken = false;
        while self.frames.len() > MAX_BUFFERED_FRAMES {
            let index = self
                .frames
                .iter()
                .position(|buffered| !buffered.frame.keyframe)
                .unwrap_or(0);
            if self
                .frames
                .remove(index)
                .is_some_and(|buffered| !buffered.frame.discardable)
            {
                reference_chain_broken = true;
            }
        }
        reference_chain_broken
    }
}

fn mapped_time_us(base_transit_us: Option<i128>, pts_us: u64) -> u64 {
    let mapped = i128::from(pts_us) + base_transit_us.unwrap_or(0);
    mapped.clamp(0, i128::from(u64::MAX)) as u64
}

fn push_bounded<T>(samples: &mut VecDeque<T>, value: T) {
    if samples.len() == SAMPLE_WINDOW {
        samples.pop_front();
    }
    samples.push_back(value);
}

fn percentile(samples: &VecDeque<u64>, percentile: usize) -> Option<u64> {
    if samples.is_empty() {
        return None;
    }
    let mut sorted = samples.iter().copied().collect::<Vec<_>>();
    sorted.sort_unstable();
    let index = ((sorted.len() - 1) * percentile) / 100;
    sorted.get(index).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(pts_us: u64, keyframe: bool) -> Frame {
        Frame {
            stream_generation: 1,
            frame_id: pts_us,
            pts_us,
            received_at_us: pts_us,
            data: Bytes::from_static(b"f"),
            keyframe,
            discardable: false,
        }
    }

    #[test]
    fn starts_at_low_latency_and_releases_before_total_target_for_decode_budget() {
        let mut buffer = JitterBuffer::new();
        assert_eq!(
            buffer.push_at(frame(1_000_000, true), 10_000, 10_000),
            PushOutcome::Accepted
        );
        assert!(buffer.pop_ready(32_999).is_none());
        assert!(buffer.pop_ready(33_000).is_some());
        assert_eq!(buffer.take_timing_stats().actual_delay_ms, 23.0);
    }

    #[test]
    fn target_grows_quickly_for_arrival_variation_and_decays_slowly() {
        let mut buffer = JitterBuffer::new();
        buffer.push_at(frame(0, true), 0, 0);
        for id in 1..=5 {
            let pts = id * 33_333;
            buffer.push_at(frame(pts, false), pts + 20_000, pts + 20_000);
        }
        let raised = buffer.target_delay_ms();
        assert!(raised >= 30.0, "target did not absorb 20ms delay: {raised}");

        buffer.push_at(frame(199_998, false), 199_998, 199_998);
        assert!(buffer.target_delay_ms() >= raised - 1.0);
    }

    #[test]
    fn rolling_transit_baseline_does_not_double_count_frame_interval_variation() {
        let mut buffer = JitterBuffer::new();
        for id in 0..150 {
            let pts = id * 33_333;
            let queueing = if id % 10 == 0 { 8_000 } else { 2_000 };
            buffer.push_at(frame(pts, id == 0), pts + queueing, pts + queueing);
        }
        assert!(
            buffer.target_delay_ms() <= 20.0,
            "healthy bounded queueing should stay in the low-latency band: {}",
            buffer.target_delay_ms()
        );
    }

    #[test]
    fn target_never_exceeds_hard_low_latency_band() {
        let mut buffer = JitterBuffer::new();
        buffer.push_at(frame(0, true), 0, 0);
        for id in 1..20 {
            let pts = id * 33_333;
            let arrival = pts + 200_000;
            let _ = buffer.push_at(frame(pts, false), arrival, arrival);
        }
        assert_eq!(buffer.target_delay_ms(), 80.0);
    }

    #[test]
    fn complete_arrival_is_not_rejected_from_unsynchronized_pts_transit() {
        let mut buffer = JitterBuffer::new();
        buffer.push_at(frame(0, true), 0, 0);
        assert_eq!(
            buffer.push_at(frame(33_333, false), 200_000, 200_000),
            PushOutcome::Accepted
        );
    }

    #[test]
    fn completed_frame_expires_only_after_waiting_in_receiver_queue() {
        let mut buffer = JitterBuffer::new();
        buffer.push_at(frame(0, true), 0, 0);
        assert!(!buffer.drop_expired(120_000, 120_000));
        assert!(buffer.drop_expired(120_001, 120_000));
        assert!(buffer.is_empty());
    }

    #[test]
    fn occupancy_is_distinct_from_target_and_discrete_at_frame_periods() {
        let mut buffer = JitterBuffer::new();
        buffer.set_fixed_target_ms(Some(100));
        buffer.push_at(frame(0, true), 0, 0);
        buffer.push_at(frame(33_333, false), 1_000, 1_000);
        buffer.push_at(frame(66_666, false), 2_000, 2_000);
        assert!((buffer.occupancy_ms() - 66.666).abs() < 0.001);
        assert_eq!(buffer.target_delay_ms(), 100.0);
    }

    #[test]
    fn fixed_zero_target_releases_immediately() {
        let mut buffer = JitterBuffer::new();
        buffer.set_fixed_target_ms(Some(0));
        buffer.push_at(frame(1_000_000, true), 4_000, 4_000);
        assert!(buffer.pop_ready(4_000).is_some());
    }

    #[test]
    fn orders_cross_access_unit_completion_by_pts() {
        let mut buffer = JitterBuffer::new();
        buffer.set_fixed_target_ms(Some(50));
        buffer.push_at(frame(34_000, false), 2_000, 2_000);
        buffer.push_at(frame(1_000, true), 3_000, 3_000);
        assert_eq!(buffer.pop_ready(100_000).unwrap().pts_us, 1_000);
        assert_eq!(buffer.pop_ready(100_000).unwrap().pts_us, 34_000);
    }

    #[test]
    fn older_access_unit_after_playout_is_rejected() {
        let mut buffer = JitterBuffer::new();
        buffer.set_fixed_target_ms(Some(0));
        buffer.push_at(frame(34_000, false), 0, 0);
        assert!(buffer.pop_ready(0).is_some());
        assert_eq!(
            buffer.push_at(frame(1_000, true), 1_000, 1_000),
            PushOutcome::DroppedLate {
                requires_refresh: true
            }
        );
    }

    #[test]
    fn capacity_covers_the_full_failure_and_playout_budget_at_30_fps() {
        let mut buffer = JitterBuffer::new();
        buffer.set_fixed_target_ms(Some(80));
        for id in 0..MAX_BUFFERED_FRAMES {
            let pts = id as u64 * 33_333;
            assert_eq!(
                buffer.push_at(frame(pts, id == 0), 0, 0),
                PushOutcome::Accepted,
            );
        }
        assert!(buffer.occupancy_ms() > 499.0);
        assert_eq!(buffer.len(), MAX_BUFFERED_FRAMES);

        let overflow_pts = MAX_BUFFERED_FRAMES as u64 * 33_333;
        assert_eq!(
            buffer.push_at(frame(overflow_pts, false), 0, 0),
            PushOutcome::AcceptedAfterReferenceDrop,
        );
        assert_eq!(buffer.len(), MAX_BUFFERED_FRAMES);
    }
}
