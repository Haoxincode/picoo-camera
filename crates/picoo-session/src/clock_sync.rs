//! Bounded NTP-style remote monotonic-clock mapping.
//!
//! Picoo transports the four timestamps over its already authenticated PCP
//! control stream, so this module intentionally implements only the small
//! product-specific estimator rather than embedding an NTP network stack
//! (REQ-PICOO-SESSION-014).

use std::collections::VecDeque;

use thiserror::Error;

pub const MAX_CLOCK_SYNC_SAMPLES: usize = 12;
const MIN_STABLE_SAMPLES: usize = 3;
const MIN_STABLE_SPAN_US: u64 = 500_000;
const MAX_ACCEPTED_ROUND_TRIP_US: u64 = 250_000;
const LOW_DELAY_BAND_US: u64 = 2_000;
const MAX_SLOPE_ERROR: f64 = 0.001;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockSyncExchange {
    pub generation: u64,
    pub local_send_us: u64,
    pub remote_receive_us: u64,
    pub remote_send_us: u64,
    pub local_receive_us: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClockMappingEstimate {
    pub local_time_us: u64,
    pub uncertainty_us: u64,
}

#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub enum ClockSyncError {
    #[error("clock sync exchange belongs to generation {got}, expected {expected}")]
    StaleGeneration { got: u64, expected: u64 },
    #[error("clock sync timestamps are not monotonic within their endpoint")]
    NonMonotonicExchange,
    #[error("remote processing interval exceeds the complete local round trip")]
    ImpossibleRoundTrip,
    #[error("clock sync round trip exceeds the bounded realtime window")]
    ExcessiveRoundTrip,
}

#[derive(Debug, Clone, Copy)]
struct SyncPoint {
    remote_midpoint_us: f64,
    local_midpoint_us: f64,
    uncertainty_us: u64,
}

#[derive(Debug, Clone)]
pub struct AffineClockMapper {
    generation: u64,
    samples: VecDeque<SyncPoint>,
    slope: f64,
    intercept_us: f64,
    uncertainty_us: u64,
    stable: bool,
}

impl AffineClockMapper {
    pub fn new(generation: u64) -> Self {
        Self {
            generation,
            samples: VecDeque::with_capacity(MAX_CLOCK_SYNC_SAMPLES),
            slope: 1.0,
            intercept_us: 0.0,
            uncertainty_us: u64::MAX,
            stable: false,
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn reset(&mut self, generation: u64) {
        self.generation = generation;
        self.samples.clear();
        self.slope = 1.0;
        self.intercept_us = 0.0;
        self.uncertainty_us = u64::MAX;
        self.stable = false;
    }

    pub fn observe(&mut self, exchange: ClockSyncExchange) -> Result<(), ClockSyncError> {
        if exchange.generation != self.generation {
            return Err(ClockSyncError::StaleGeneration {
                got: exchange.generation,
                expected: self.generation,
            });
        }
        if exchange.remote_send_us < exchange.remote_receive_us
            || exchange.local_receive_us < exchange.local_send_us
        {
            return Err(ClockSyncError::NonMonotonicExchange);
        }
        let local_elapsed = exchange.local_receive_us - exchange.local_send_us;
        let remote_processing = exchange.remote_send_us - exchange.remote_receive_us;
        if remote_processing > local_elapsed {
            return Err(ClockSyncError::ImpossibleRoundTrip);
        }
        let round_trip_us = local_elapsed - remote_processing;
        if round_trip_us > MAX_ACCEPTED_ROUND_TRIP_US {
            return Err(ClockSyncError::ExcessiveRoundTrip);
        }

        let point = SyncPoint {
            remote_midpoint_us: midpoint(exchange.remote_receive_us, exchange.remote_send_us),
            local_midpoint_us: midpoint(exchange.local_send_us, exchange.local_receive_us),
            uncertainty_us: round_trip_us.div_ceil(2),
        };
        if self.samples.len() == MAX_CLOCK_SYNC_SAMPLES {
            self.samples.pop_front();
        }
        self.samples.push_back(point);
        self.recompute();
        Ok(())
    }

    pub fn is_stable(&self) -> bool {
        self.stable
    }

    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }

    pub fn slope(&self) -> Option<f64> {
        self.stable.then_some(self.slope)
    }

    pub fn estimate_local_time(&self, remote_time_us: u64) -> Option<ClockMappingEstimate> {
        if !self.stable {
            return None;
        }
        let mapped = self.slope.mul_add(remote_time_us as f64, self.intercept_us);
        if !mapped.is_finite() || mapped < 0.0 || mapped > u64::MAX as f64 {
            return None;
        }
        Some(ClockMappingEstimate {
            local_time_us: mapped.round() as u64,
            uncertainty_us: self.uncertainty_us,
        })
    }

    fn recompute(&mut self) {
        self.stable = false;
        let Some(min_uncertainty) = self
            .samples
            .iter()
            .map(|sample| sample.uncertainty_us)
            .min()
        else {
            return;
        };
        let accepted: Vec<_> = self
            .samples
            .iter()
            .copied()
            .filter(|sample| {
                sample.uncertainty_us <= min_uncertainty.saturating_add(LOW_DELAY_BAND_US)
            })
            .collect();
        if accepted.len() < MIN_STABLE_SAMPLES {
            return;
        }

        let first_remote = accepted[0].remote_midpoint_us;
        let first_local = accepted[0].local_midpoint_us;
        let remote_span = accepted
            .last()
            .map_or(0.0, |last| last.remote_midpoint_us - first_remote);
        if remote_span < MIN_STABLE_SPAN_US as f64 {
            return;
        }

        let count = accepted.len() as f64;
        let mean_x = accepted
            .iter()
            .map(|sample| sample.remote_midpoint_us - first_remote)
            .sum::<f64>()
            / count;
        let mean_y = accepted
            .iter()
            .map(|sample| sample.local_midpoint_us - first_local)
            .sum::<f64>()
            / count;
        let covariance = accepted
            .iter()
            .map(|sample| {
                let x = sample.remote_midpoint_us - first_remote - mean_x;
                let y = sample.local_midpoint_us - first_local - mean_y;
                x * y
            })
            .sum::<f64>();
        let variance = accepted
            .iter()
            .map(|sample| {
                let x = sample.remote_midpoint_us - first_remote - mean_x;
                x * x
            })
            .sum::<f64>();
        if variance <= f64::EPSILON {
            return;
        }
        let slope = (covariance / variance).clamp(1.0 - MAX_SLOPE_ERROR, 1.0 + MAX_SLOPE_ERROR);
        let intercept = (first_local + mean_y) - slope * (first_remote + mean_x);
        let residual_us = accepted
            .iter()
            .map(|sample| {
                (slope.mul_add(sample.remote_midpoint_us, intercept) - sample.local_midpoint_us)
                    .abs()
            })
            .fold(0.0, f64::max)
            .ceil() as u64;

        self.slope = slope;
        self.intercept_us = intercept;
        self.uncertainty_us = accepted
            .iter()
            .map(|sample| sample.uncertainty_us)
            .max()
            .unwrap_or(u64::MAX)
            .saturating_add(residual_us);
        self.stable = true;
    }
}

fn midpoint(first: u64, second: u64) -> f64 {
    first as f64 + second.saturating_sub(first) as f64 / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exchange(
        generation: u64,
        remote_midpoint_us: u64,
        slope: f64,
        intercept_us: f64,
        one_way_delay_us: u64,
    ) -> ClockSyncExchange {
        let local_midpoint = slope.mul_add(remote_midpoint_us as f64, intercept_us) as u64;
        ClockSyncExchange {
            generation,
            local_send_us: local_midpoint - one_way_delay_us,
            remote_receive_us: remote_midpoint_us,
            remote_send_us: remote_midpoint_us,
            local_receive_us: local_midpoint + one_way_delay_us,
        }
    }

    #[test]
    fn stable_mapping_recovers_offset_and_drift() {
        let mut mapper = AffineClockMapper::new(4);
        for index in 0..6 {
            mapper
                .observe(exchange(
                    4,
                    1_000_000 + index * 300_000,
                    1.000_2,
                    75_000.0,
                    2_000,
                ))
                .expect("valid exchange");
        }
        assert!(mapper.is_stable());
        assert!((mapper.slope().expect("slope") - 1.000_2).abs() < 0.000_01);
        let estimate = mapper.estimate_local_time(4_000_000).expect("mapping");
        assert!(estimate.local_time_us.abs_diff(4_075_800) <= 2);
        assert_eq!(estimate.uncertainty_us, 2_000);
    }

    #[test]
    fn high_delay_outlier_does_not_move_low_delay_fit() {
        let mut mapper = AffineClockMapper::new(9);
        for index in 0..5 {
            mapper
                .observe(exchange(
                    9,
                    2_000_000 + index * 300_000,
                    1.0,
                    10_000.0,
                    1_000,
                ))
                .expect("low delay exchange");
        }
        mapper
            .observe(exchange(9, 3_500_000, 1.0, 50_000.0, 30_000))
            .expect("bounded outlier");
        let estimate = mapper.estimate_local_time(4_000_000).expect("mapping");
        assert!(estimate.local_time_us.abs_diff(4_010_000) <= 2);
    }

    #[test]
    fn mapping_is_hidden_until_sample_count_and_span_are_sufficient() {
        let mut mapper = AffineClockMapper::new(1);
        for index in 0..2 {
            mapper
                .observe(exchange(1, 100_000 + index * 100_000, 1.0, 0.0, 500))
                .expect("sample");
        }
        assert!(!mapper.is_stable());
        assert_eq!(mapper.estimate_local_time(1), None);
    }

    #[test]
    fn generation_reset_invalidates_previous_mapping() {
        let mut mapper = AffineClockMapper::new(1);
        for index in 0..3 {
            mapper
                .observe(exchange(1, 100_000 + index * 300_000, 1.0, 0.0, 500))
                .expect("sample");
        }
        assert!(mapper.is_stable());
        mapper.reset(2);
        assert!(!mapper.is_stable());
        assert_eq!(mapper.sample_count(), 0);
        assert!(matches!(
            mapper.observe(exchange(1, 2_000_000, 1.0, 0.0, 500)),
            Err(ClockSyncError::StaleGeneration {
                got: 1,
                expected: 2
            })
        ));
    }

    #[test]
    fn invalid_exchange_is_rejected_without_poisoning_samples() {
        let mut mapper = AffineClockMapper::new(1);
        assert_eq!(
            mapper.observe(ClockSyncExchange {
                generation: 1,
                local_send_us: 20,
                remote_receive_us: 50,
                remote_send_us: 49,
                local_receive_us: 30,
            }),
            Err(ClockSyncError::NonMonotonicExchange)
        );
        assert_eq!(mapper.sample_count(), 0);

        assert_eq!(
            mapper.observe(ClockSyncExchange {
                generation: 1,
                local_send_us: 100,
                remote_receive_us: 1_000,
                remote_send_us: 1_100,
                local_receive_us: 150,
            }),
            Err(ClockSyncError::ImpossibleRoundTrip)
        );
        assert_eq!(mapper.sample_count(), 0);
    }
}
