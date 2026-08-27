//! Session states and reconnect backoff — REQ-PICOO-SESSION-001, REQ-PICOO-TRANSPORT-004.

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ReceiverStatus {
    Discovering,
    Pairing,
    Connecting,
    Negotiating,
    Streaming,
    Reconnecting,
    #[default]
    Disconnected,
    PermissionRequired,
    VirtualCameraUnavailable,
    NetworkUnstable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SenderStatus {
    Discovering,
    Pairing,
    Connecting,
    Negotiating,
    Streaming,
    Reconnecting,
    Disconnected,
    PermissionRequired,
    NetworkUnstable,
}

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("invalid transition from {from:?} to {to:?}")]
    InvalidTransition { from: ReceiverStatus, to: ReceiverStatus },
}

#[derive(Default)]
pub struct ReconnectBackoff {
    attempt: u32,
}

impl ReconnectBackoff {
    /// Backoff schedule: 500ms, 1s, 2s, 5s, then 5s forever.
    pub fn next_delay_ms(&mut self) -> u64 {
        let delay = match self.attempt {
            0 => 500,
            1 => 1_000,
            2 => 2_000,
            _ => 5_000,
        };
        self.attempt += 1;
        delay
    }

    pub fn reset(&mut self) {
        self.attempt = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconnect_backoff_schedule() {
        let mut backoff = ReconnectBackoff::default();
        assert_eq!(backoff.next_delay_ms(), 500);
        assert_eq!(backoff.next_delay_ms(), 1_000);
        assert_eq!(backoff.next_delay_ms(), 2_000);
        assert_eq!(backoff.next_delay_ms(), 5_000);
        assert_eq!(backoff.next_delay_ms(), 5_000);
        backoff.reset();
        assert_eq!(backoff.next_delay_ms(), 500);
    }
}
