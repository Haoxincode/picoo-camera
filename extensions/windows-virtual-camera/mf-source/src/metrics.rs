//! Per-window Media Foundation sample-delivery telemetry.
//!
//! REQ-PICOO-VCAM-008. The metrics intentionally observe current Frame Server
//! behavior without imposing a pacing policy before Win11 evidence exists.

use std::time::{Duration, Instant};

use crate::frame_provider::FrameOrigin;

const REPORT_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VcamMetricsSnapshot {
    pub elapsed_ms: u64,
    pub requests: u64,
    pub fresh: u64,
    pub cached: u64,
    pub placeholder: u64,
    pub failed: u64,
    pub delivery_average_us: u64,
    pub delivery_max_us: u64,
}

pub(crate) struct VcamMetrics {
    window_started: Instant,
    requests: u64,
    fresh: u64,
    cached: u64,
    placeholder: u64,
    failed: u64,
    delivery_total_us: u64,
    delivery_max_us: u64,
}

impl VcamMetrics {
    pub fn new() -> Self {
        Self {
            window_started: Instant::now(),
            requests: 0,
            fresh: 0,
            cached: 0,
            placeholder: 0,
            failed: 0,
            delivery_total_us: 0,
            delivery_max_us: 0,
        }
    }

    pub fn record_result(
        &mut self,
        origin: Option<FrameOrigin>,
        delivery_time: Duration,
    ) -> Option<VcamMetricsSnapshot> {
        self.requests = self.requests.saturating_add(1);
        match origin {
            Some(FrameOrigin::Fresh) => self.fresh = self.fresh.saturating_add(1),
            Some(FrameOrigin::Cached) => self.cached = self.cached.saturating_add(1),
            Some(FrameOrigin::Placeholder) => {
                self.placeholder = self.placeholder.saturating_add(1);
            }
            None => self.failed = self.failed.saturating_add(1),
        }
        self.record_delivery_time(delivery_time);
        self.take_snapshot_if_due()
    }

    fn record_delivery_time(&mut self, delivery_time: Duration) {
        let delivery_us = delivery_time.as_micros().min(u64::MAX as u128) as u64;
        self.delivery_total_us = self.delivery_total_us.saturating_add(delivery_us);
        self.delivery_max_us = self.delivery_max_us.max(delivery_us);
    }

    fn take_snapshot_if_due(&mut self) -> Option<VcamMetricsSnapshot> {
        let elapsed = self.window_started.elapsed();
        if elapsed < REPORT_INTERVAL {
            return None;
        }

        let snapshot = VcamMetricsSnapshot {
            elapsed_ms: elapsed.as_millis().min(u64::MAX as u128) as u64,
            requests: self.requests,
            fresh: self.fresh,
            cached: self.cached,
            placeholder: self.placeholder,
            failed: self.failed,
            delivery_average_us: self.delivery_total_us / self.requests.max(1),
            delivery_max_us: self.delivery_max_us,
        };
        self.window_started = Instant::now();
        self.requests = 0;
        self.fresh = 0;
        self.cached = 0;
        self.placeholder = 0;
        self.failed = 0;
        self.delivery_total_us = 0;
        self.delivery_max_us = 0;
        Some(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_keeps_request_and_frame_origin_counts_consistent() {
        let mut metrics = VcamMetrics::new();
        metrics.window_started = Instant::now() - REPORT_INTERVAL;

        assert!(metrics
            .record_result(Some(FrameOrigin::Fresh), Duration::from_micros(100))
            .is_some_and(|snapshot| {
                snapshot.requests == 1
                    && snapshot.fresh == 1
                    && snapshot.cached == 0
                    && snapshot.placeholder == 0
                    && snapshot.failed == 0
                    && snapshot.delivery_average_us == 100
                    && snapshot.delivery_max_us == 100
            }));
    }

    #[test]
    fn report_window_resets_after_snapshot() {
        let mut metrics = VcamMetrics::new();
        metrics.window_started = Instant::now() - REPORT_INTERVAL;
        let _ = metrics.record_result(Some(FrameOrigin::Cached), Duration::from_micros(200));

        assert!(metrics
            .record_result(Some(FrameOrigin::Placeholder), Duration::from_micros(50))
            .is_none());
    }

    #[test]
    fn failed_request_is_visible_without_a_frame_origin() {
        let mut metrics = VcamMetrics::new();
        metrics.window_started = Instant::now() - REPORT_INTERVAL;

        let snapshot = metrics
            .record_result(None, Duration::from_micros(25))
            .expect("snapshot");
        assert_eq!(snapshot.requests, 1);
        assert_eq!(snapshot.failed, 1);
        assert_eq!(snapshot.fresh + snapshot.cached + snapshot.placeholder, 0);
    }
}
