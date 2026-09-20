//! Monotonic Media Foundation sample timeline — REQ-PICOO-VCAM-011.

#[derive(Debug, Clone)]
pub(crate) struct SampleClock {
    period_100ns_num: i128,
    rate_num: i128,
    anchor_100ns: Option<i128>,
    last_slot: Option<i128>,
}

impl SampleClock {
    #[cfg(test)]
    pub(crate) const fn new(duration_100ns: i64) -> Self {
        assert!(duration_100ns > 0, "sample duration must be positive");
        Self {
            period_100ns_num: duration_100ns as i128,
            rate_num: 1,
            anchor_100ns: None,
            last_slot: None,
        }
    }

    pub(crate) fn for_frame_rate(rate_num: u32, rate_den: u32) -> Option<Self> {
        if rate_num == 0 || rate_den == 0 {
            return None;
        }
        Some(Self {
            period_100ns_num: 10_000_000_i128.checked_mul(i128::from(rate_den))?,
            rate_num: i128::from(rate_num),
            anchor_100ns: None,
            last_slot: None,
        })
    }

    pub(crate) fn reset(&mut self) {
        self.anchor_100ns = None;
        self.last_slot = None;
    }

    /// Allocate one timestamp without sleeping or accumulating missed samples.
    ///
    /// Fast requesters advance by exactly one frame duration. Slow requesters
    /// skip elapsed output slots and receive the newest slot not after `now`.
    /// A backwards platform clock cannot make the media timeline regress.
    pub(crate) fn next_timestamp(&mut self, now_100ns: i64) -> Option<i64> {
        let now = i128::from(now_100ns);
        let (anchor, next_slot) = match (self.anchor_100ns, self.last_slot) {
            (Some(anchor), Some(last_slot)) => (anchor, last_slot.checked_add(1)?),
            _ => {
                self.anchor_100ns = Some(now);
                self.last_slot = Some(0);
                return Some(now_100ns);
            }
        };

        // Pick the newest absolute slot that is not in the future. Computing
        // from the anchor avoids accumulating a truncated 100ns duration.
        let elapsed = now.checked_sub(anchor).unwrap_or_default();
        let elapsed_slot = elapsed
            .checked_mul(self.rate_num)?
            .checked_div(self.period_100ns_num)?;
        let slot = next_slot.max(elapsed_slot);
        let timestamp = anchor.checked_add(
            slot.checked_mul(self.period_100ns_num)?
                .checked_div(self.rate_num)?,
        )?;
        let timestamp = i64::try_from(timestamp).ok()?;
        self.last_slot = Some(slot);
        Some(timestamp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: i64 = 333_333;

    #[test]
    fn first_sample_anchors_to_platform_time() {
        let mut clock = SampleClock::new(FRAME);
        assert_eq!(clock.next_timestamp(7_000_000), Some(7_000_000));
    }

    #[test]
    fn fast_requests_advance_exactly_one_frame() {
        let mut clock = SampleClock::new(FRAME);
        let first = clock.next_timestamp(1_000_000).expect("first");
        let second = clock.next_timestamp(1_000_001).expect("second");
        let third = clock.next_timestamp(1_000_002).expect("third");
        assert_eq!(second - first, FRAME);
        assert_eq!(third - second, FRAME);
    }

    #[test]
    fn slow_requests_skip_missed_slots_without_catch_up() {
        let mut clock = SampleClock::new(FRAME);
        let first = clock.next_timestamp(0).expect("first");
        let now = FRAME * 10 + 123;
        let after_stall = clock.next_timestamp(now).expect("after stall");
        assert_eq!(first, 0);
        assert_eq!(after_stall, FRAME * 10);
        assert!(now - after_stall < FRAME);
        assert_eq!(clock.next_timestamp(now + 1), Some(FRAME * 11));
    }

    #[test]
    fn backwards_platform_time_remains_monotonic() {
        let mut clock = SampleClock::new(FRAME);
        let first = clock.next_timestamp(5_000_000).expect("first");
        let second = clock.next_timestamp(4_000_000).expect("second");
        assert_eq!(second, first + FRAME);
    }

    #[test]
    fn reset_reanchors_after_stream_restart() {
        let mut clock = SampleClock::new(FRAME);
        let _ = clock.next_timestamp(5_000_000);
        let _ = clock.next_timestamp(5_000_001);
        clock.reset();
        assert_eq!(clock.next_timestamp(9_000_000), Some(9_000_000));
    }

    #[test]
    fn timestamp_exhaustion_fails_instead_of_repeating() {
        let mut clock = SampleClock::new(FRAME);
        assert_eq!(clock.next_timestamp(i64::MAX), Some(i64::MAX));
        assert_eq!(clock.next_timestamp(i64::MAX), None);
    }

    #[test]
    fn rational_rate_does_not_accumulate_truncated_duration() {
        let mut clock = SampleClock::for_frame_rate(30, 1).expect("30 fps");
        let first = clock.next_timestamp(0).expect("first");
        let second = clock.next_timestamp(1).expect("second");
        let third = clock.next_timestamp(2).expect("third");
        assert_eq!(first, 0);
        assert_eq!(second, 333_333);
        assert_eq!(third, 666_666);

        for _ in 0..27 {
            let _ = clock.next_timestamp(3);
        }
        let thirtieth = clock.next_timestamp(4).expect("thirtieth");
        assert_eq!(thirtieth, 10_000_000);
        let thirty_first = clock.next_timestamp(5).expect("thirty-first");
        assert_eq!(thirty_first, 10_333_333);
    }

    #[test]
    fn sixty_fps_uses_half_frame_period() {
        let mut clock = SampleClock::for_frame_rate(60, 1).expect("60 fps");
        let first = clock.next_timestamp(0).expect("first");
        let second = clock.next_timestamp(1).expect("second");
        assert_eq!(second - first, 166_666);
    }
}
