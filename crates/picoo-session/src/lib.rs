//! Session states and reconnect backoff — REQ-PICOO-SESSION-001, REQ-PICOO-TRANSPORT-004.

mod clock_sync;
mod network_health;
mod runtime_state;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use clock_sync::{
    AffineClockMapper, ClockMappingEstimate, ClockSyncError, ClockSyncExchange,
    MAX_CLOCK_SYNC_SAMPLES,
};
pub use network_health::{
    NetworkDegradation, NetworkEpisode, NetworkHealth, NetworkHealthTracker,
    BAD_PACKET_LOSS_THRESHOLD, BAD_WINDOWS_TO_ENTER, CLEAN_PACKET_LOSS_THRESHOLD,
    CLEAN_WINDOWS_TO_RECOVER,
};
pub use runtime_state::{
    ConnectionState, HealthState, OutputState, SessionRuntimeState, StreamState, TrustState,
};

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

impl ReceiverStatus {
    /// Stable UI label (REQ-PICOO-SESSION-001).
    pub fn as_label(self) -> &'static str {
        match self {
            Self::Discovering => "Discovering",
            Self::Pairing => "Pairing",
            Self::Connecting => "Connecting",
            Self::Negotiating => "Negotiating",
            Self::Streaming => "Streaming",
            Self::Reconnecting => "Reconnecting",
            Self::Disconnected => "Disconnected",
            Self::PermissionRequired => "Permission Required",
            Self::VirtualCameraUnavailable => "Virtual Camera Unavailable",
            Self::NetworkUnstable => "Network Unstable",
        }
    }
}

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SenderStatus {
    Disconnected = 0,
    Discovering = 1,
    Pairing = 2,
    Connecting = 3,
    Negotiating = 4,
    Streaming = 5,
    Reconnecting = 6,
    PermissionRequired = 7,
    NetworkUnstable = 8,
}

impl SenderStatus {
    pub const ALL: [Self; 9] = [
        Self::Disconnected,
        Self::Discovering,
        Self::Pairing,
        Self::Connecting,
        Self::Negotiating,
        Self::Streaming,
        Self::Reconnecting,
        Self::PermissionRequired,
        Self::NetworkUnstable,
    ];

    /// Stable UI label (REQ-PICOO-SESSION-001).
    pub fn as_label(self) -> &'static str {
        match self {
            Self::Discovering => "Discovering",
            Self::Pairing => "Pairing",
            Self::Connecting => "Connecting",
            Self::Negotiating => "Negotiating",
            Self::Streaming => "Streaming",
            Self::Reconnecting => "Reconnecting",
            Self::Disconnected => "Disconnected",
            Self::PermissionRequired => "Permission Required",
            Self::NetworkUnstable => "Network Unstable",
        }
    }

    /// Stable FFI / JNI status code shared by generated platform bindings.
    pub fn as_code(self) -> i32 {
        self as i32
    }
}

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("invalid transition from {from:?} to {to:?}")]
    InvalidTransition {
        from: ReceiverStatus,
        to: ReceiverStatus,
    },
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

    /// Attempts since last successful connect (1-based after first [`Self::next_delay_ms`]).
    pub fn attempt(&self) -> u32 {
        self.attempt
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

    #[test]
    fn session_status_labels_cover_arch_set() {
        // REQ-PICOO-SESSION-001 — ARCH-PICOO-SESSION-001 required statuses.
        let receiver = [
            ReceiverStatus::Discovering,
            ReceiverStatus::Pairing,
            ReceiverStatus::Connecting,
            ReceiverStatus::Negotiating,
            ReceiverStatus::Streaming,
            ReceiverStatus::Reconnecting,
            ReceiverStatus::Disconnected,
            ReceiverStatus::PermissionRequired,
            ReceiverStatus::VirtualCameraUnavailable,
            ReceiverStatus::NetworkUnstable,
        ];
        assert_eq!(receiver.len(), 10);
        for status in receiver {
            assert!(!status.as_label().is_empty());
        }

        let sender = [
            SenderStatus::Discovering,
            SenderStatus::Pairing,
            SenderStatus::Connecting,
            SenderStatus::Negotiating,
            SenderStatus::Streaming,
            SenderStatus::Reconnecting,
            SenderStatus::Disconnected,
            SenderStatus::PermissionRequired,
            SenderStatus::NetworkUnstable,
        ];
        assert_eq!(sender.len(), 9);
        for status in sender {
            assert!(!status.as_label().is_empty());
            assert!((0..=8).contains(&status.as_code()));
        }
        assert_eq!(SenderStatus::PermissionRequired.as_code(), 7);
        assert_eq!(SenderStatus::NetworkUnstable.as_code(), 8);
    }

    #[test]
    fn sender_and_receiver_labels_are_stable_english() {
        assert_eq!(
            ReceiverStatus::VirtualCameraUnavailable.as_label(),
            "Virtual Camera Unavailable"
        );
        assert_eq!(
            SenderStatus::PermissionRequired.as_label(),
            "Permission Required"
        );
        assert_eq!(SenderStatus::NetworkUnstable.as_label(), "Network Unstable");
    }
}
