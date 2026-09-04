//! Orthogonal Receiver network health projection — REQ-PICOO-SESSION-013.

use std::time::Instant;

use picoo_session::{HealthState, NetworkHealth, ReceiverStatus};

use super::ReceiverSession;

impl ReceiverSession {
    /// Compatibility display projection derived from orthogonal Core state.
    pub fn status(&self) -> ReceiverStatus {
        self.runtime_state.receiver_status()
    }

    pub fn network_health(&self) -> &NetworkHealth {
        self.network_health.health()
    }

    pub(super) fn observe_network_packet_loss(&mut self, packet_loss: f64) {
        self.network_health
            .observe_packet_loss(packet_loss, Instant::now());
        self.runtime_state
            .set_health(if self.network_health.health().is_degraded() {
                HealthState::NetworkDegraded
            } else {
                HealthState::Healthy
            });
    }

    pub(super) fn reset_network_health(&mut self) {
        self.network_health.reset();
        self.runtime_state.set_health(HealthState::Healthy);
    }

    #[cfg(test)]
    pub(crate) fn observe_network_packet_loss_for_test(&mut self, packet_loss: f64) {
        self.observe_network_packet_loss(packet_loss);
    }

    #[cfg(test)]
    pub(crate) fn lifecycle_status_for_test(&self) -> ReceiverStatus {
        if self.runtime_state.stream().is_streaming() {
            ReceiverStatus::Streaming
        } else {
            self.runtime_state.receiver_status()
        }
    }
}
