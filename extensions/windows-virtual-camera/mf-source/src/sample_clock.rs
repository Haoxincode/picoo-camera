//! Monotonic Media Foundation sample timeline — REQ-PICOO-VCAM-011.

#[derive(Debug, Clone)]
pub(crate) struct SampleClock {
    duration_100ns: i64,
    last_pts_100ns: Option<i64>,
}

impl SampleClock {
    pub(crate) const fn new(duration_100ns: i64) -> Self {
        assert!(duration_100ns > 0, "sample duration must be positive");
        Self {
            duration_100ns,
            last_pts_100ns: None,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.last_pts_100ns = None;
    }

    /// Allocate one timestamp without sleeping or accumulating missed samples.
    ///
    /// Fast requesters advance by exactly one frame duration. Slow requesters
    /// skip elapsed output slots and receive the newest slot not after `now`.
    /// A backwards platform clock cannot make the media timeline regress.
    pub(crate) fn next_timestamp(&mut self, now_100ns: i64) -> Option<i64> {
        let Some(last) = self.last_pts_100ns else {
            self.last_pts_100ns = Some(now_100ns);
            return Some(now_100ns);
        };

        let next = i128::from(last) + i128::from(self.duration_100ns);
        let now = i128::from(now_100ns);
        let timestamp = if now <= next {
            next
        } else {
            let elapsed = now - next;
            let skipped = elapsed / i128::from(self.duration_100ns);
            next + skipped * i128::from(self.duration_100ns)
        };
        let timestamp = i64::try_from(timestamp).ok()?;
        self.last_pts_100ns = Some(timestamp);
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
}
