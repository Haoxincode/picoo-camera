use std::time::{Duration, Instant};

/// Monotonic clock advanced only by the simulation driver.
#[derive(Debug, Clone)]
pub struct VirtualClock {
    origin: Instant,
    now_us: u64,
}

impl Default for VirtualClock {
    fn default() -> Self {
        Self::new()
    }
}

impl VirtualClock {
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
            now_us: 0,
        }
    }

    pub fn now_us(&self) -> u64 {
        self.now_us
    }

    pub fn instant(&self) -> Instant {
        self.origin + Duration::from_micros(self.now_us)
    }

    pub fn micros_since_origin(&self, instant: Instant) -> u64 {
        instant
            .checked_duration_since(self.origin)
            .map_or(0, |duration| {
                duration.as_micros().min(u128::from(u64::MAX)) as u64
            })
    }

    pub fn advance(&mut self, duration: Duration) {
        self.now_us = self
            .now_us
            .saturating_add(duration.as_micros().min(u128::from(u64::MAX)) as u64);
    }

    pub fn advance_to_us(&mut self, target_us: u64) {
        self.now_us = self.now_us.max(target_us);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_time_never_moves_backwards() {
        let mut clock = VirtualClock::new();
        clock.advance(Duration::from_millis(3));
        clock.advance_to_us(2_000);
        assert_eq!(clock.now_us(), 3_000);
        clock.advance_to_us(5_000);
        assert_eq!(clock.now_us(), 5_000);
    }
}
