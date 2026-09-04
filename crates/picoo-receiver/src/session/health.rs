//! Orthogonal Receiver network health projection — REQ-PICOO-SESSION-013.

use std::time::Instant;

use picoo_session::{NetworkHealth, ReceiverStatus};

use super::ReceiverSession;

impl ReceiverSession {
    /// Compatibility display projection. The lifecycle state remains
    /// `Streaming`; network degradation is an independent health dimension.
    pub fn status(&self) -> ReceiverStatus {
        if self.status == ReceiverStatus::Streaming && self.network_health.health().is_degraded() {
            ReceiverStatus::NetworkUnstable
        } else {
            self.status
        }
    }

    pub fn network_health(&self) -> &NetworkHealth {
        self.network_health.health()
    }

    pub(super) fn observe_network_packet_loss(&mut self, packet_loss: f64) {
        self.network_health
            .observe_packet_loss(packet_loss, Instant::now());
    }

    pub(super) fn reset_network_health(&mut self) {
        self.network_health.reset();
    }

    #[cfg(test)]
    pub(crate) fn observe_network_packet_loss_for_test(&mut self, packet_loss: f64) {
        self.observe_network_packet_loss(packet_loss);
    }

    #[cfg(test)]
    pub(crate) fn lifecycle_status_for_test(&self) -> ReceiverStatus {
        self.status
    }
}
