//! Nonblocking observation, not permission to destroy native objects — MEDIA-076.
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

const STALL_THRESHOLD: Duration = Duration::from_secs(15);

pub(super) struct Progress {
    origin: Instant,
    last_tick_ms: AtomicU64,
}

impl Progress {
    pub(super) fn new() -> Self {
        Self {
            origin: Instant::now(),
            last_tick_ms: AtomicU64::new(0),
        }
    }

    pub(super) fn tick(&self) {
        self.tick_at(Instant::now());
    }

    fn tick_at(&self, now: Instant) {
        self.last_tick_ms
            .store(self.elapsed_ms(now), Ordering::Release);
    }

    pub(super) fn stalled(&self) -> bool {
        self.stalled_at(Instant::now())
    }

    fn stalled_at(&self, now: Instant) -> bool {
        self.elapsed_ms(now)
            .saturating_sub(self.last_tick_ms.load(Ordering::Acquire))
            >= STALL_THRESHOLD.as_millis() as u64
    }

    fn elapsed_ms(&self, now: Instant) -> u64 {
        u64::try_from(now.saturating_duration_since(self.origin).as_millis()).unwrap_or(u64::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stalled_worker_can_be_observed_without_waiting_for_it_and_can_recover() {
        let progress = std::sync::Arc::new(Progress::new());
        let worker_progress = progress.clone();
        let (release, blocked) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            // Model a native call that has not returned. Observation must not
            // acquire a lock owned by this worker or wait on its completion.
            blocked.recv().unwrap();
            worker_progress.tick_at(worker_progress.origin + Duration::from_secs(16));
        });
        assert!(!progress.stalled_at(progress.origin + Duration::from_millis(14_999)));
        assert!(progress.stalled_at(progress.origin + STALL_THRESHOLD));
        assert!(progress.stalled_at(progress.origin + Duration::from_secs(16)));
        release.send(()).unwrap();
        worker.join().unwrap();
        assert!(!progress.stalled_at(progress.origin + Duration::from_secs(16)));
        assert!(!progress.stalled_at(progress.origin + Duration::from_secs(30)));
        assert!(progress.stalled_at(progress.origin + Duration::from_secs(31)));
    }
}
