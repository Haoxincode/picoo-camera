//! Blocking event notification for native platform adapters.
//!
//! The QUIC actor remains async. Mobile shells wait on this small revisioned
//! condition variable from a background executor, then call the synchronous
//! Session pump. A revision prevents an event consumed by a concurrent encoder
//! callback from becoming a lost wakeup (REQ-PICOO-TRANSPORT-009).

use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

#[derive(Debug, Default)]
struct WakeState {
    revision: u64,
}

#[derive(Debug, Default)]
struct WakeInner {
    state: Mutex<WakeState>,
    changed: Condvar,
}

#[derive(Clone, Debug, Default)]
pub struct TransportEventWake {
    inner: Arc<WakeInner>,
}

impl TransportEventWake {
    /// Advance the revision and wake every waiter. Runtime owners use the same
    /// signal for transport, decoder completion, and control-command ingress.
    pub fn signal(&self) {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.revision = state.revision.saturating_add(1);
        drop(state);
        self.inner.changed.notify_all();
    }

    pub(crate) fn notify(&self) {
        self.signal();
    }

    pub fn revision(&self) -> u64 {
        self.inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .revision
    }

    /// Wait until an event newer than `after_revision` exists or the
    /// maintenance timeout expires. The returned revision is always safe to
    /// feed into the next call.
    pub fn wait_after(&self, after_revision: u64, timeout: Duration) -> u64 {
        let state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.revision != after_revision {
            return state.revision;
        }
        let (state, _) = self
            .inner
            .changed
            .wait_timeout_while(state, timeout, |state| state.revision == after_revision)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.revision
    }
}

#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::Instant;

    use super::*;

    #[test]
    fn event_before_wait_is_not_lost() {
        let wake = TransportEventWake::default();
        wake.notify();
        assert_eq!(wake.wait_after(0, Duration::from_secs(1)), 1);
    }

    #[test]
    fn concurrent_event_wakes_before_maintenance_timeout() {
        let wake = TransportEventWake::default();
        let notifier = wake.clone();
        let thread = thread::spawn(move || {
            thread::sleep(Duration::from_millis(20));
            notifier.notify();
        });
        let started = Instant::now();
        assert_eq!(wake.wait_after(0, Duration::from_secs(5)), 1);
        assert!(started.elapsed() < Duration::from_secs(4));
        thread.join().expect("notifier");
    }

    #[test]
    fn consumed_transport_event_still_advances_platform_revision() {
        let wake = TransportEventWake::default();
        wake.notify();
        assert_eq!(wake.revision(), 1);
        assert_eq!(wake.wait_after(0, Duration::ZERO), 1);
        assert_eq!(wake.wait_after(1, Duration::ZERO), 1);
    }
}
