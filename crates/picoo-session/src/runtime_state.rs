//! Orthogonal runtime state shared by Sender and Receiver — REQ-PICOO-SESSION-011.

use serde::{Deserialize, Serialize};

use crate::{ReceiverStatus, SenderStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ConnectionState {
    #[default]
    Idle,
    Listening,
    Connecting,
    Connected {
        generation: u64,
    },
    Reconnecting {
        attempt: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TrustState {
    #[default]
    Unknown,
    Pairing,
    Authenticated,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum StreamState {
    #[default]
    Idle,
    Negotiating,
    AwaitingRefresh {
        generation: u32,
    },
    Streaming {
        generation: u32,
    },
    Stopping,
}

impl StreamState {
    pub fn is_streaming(self) -> bool {
        matches!(self, Self::Streaming { .. } | Self::AwaitingRefresh { .. })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum OutputState {
    #[default]
    Ready,
    PermissionRequired,
    VirtualCameraUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum HealthState {
    #[default]
    Healthy,
    NetworkDegraded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SessionRuntimeState {
    connection: ConnectionState,
    trust: TrustState,
    stream: StreamState,
    output: OutputState,
    health: HealthState,
}

impl SessionRuntimeState {
    pub fn connection(self) -> ConnectionState {
        self.connection
    }

    pub fn trust(self) -> TrustState {
        self.trust
    }

    pub fn stream(self) -> StreamState {
        self.stream
    }

    pub fn output(self) -> OutputState {
        self.output
    }

    pub fn health(self) -> HealthState {
        self.health
    }

    pub fn set_connection(&mut self, connection: ConnectionState) {
        self.connection = connection;
    }

    pub fn set_trust(&mut self, trust: TrustState) {
        self.trust = trust;
    }

    pub fn set_stream(&mut self, stream: StreamState) {
        self.stream = stream;
    }

    pub fn set_output(&mut self, output: OutputState) {
        self.output = output;
    }

    pub fn set_health(&mut self, health: HealthState) {
        self.health = health;
    }

    pub fn reset_session(&mut self, connection: ConnectionState) {
        self.connection = connection;
        self.trust = TrustState::Unknown;
        self.stream = StreamState::Idle;
        self.health = HealthState::Healthy;
    }

    pub fn receiver_status(self) -> ReceiverStatus {
        if self.output == OutputState::PermissionRequired {
            return ReceiverStatus::PermissionRequired;
        }
        match self.connection {
            ConnectionState::Idle => {
                if self.output == OutputState::VirtualCameraUnavailable {
                    ReceiverStatus::VirtualCameraUnavailable
                } else {
                    ReceiverStatus::Disconnected
                }
            }
            ConnectionState::Listening => {
                if self.output == OutputState::VirtualCameraUnavailable {
                    ReceiverStatus::VirtualCameraUnavailable
                } else {
                    ReceiverStatus::Discovering
                }
            }
            ConnectionState::Connecting => ReceiverStatus::Connecting,
            ConnectionState::Reconnecting { .. } => ReceiverStatus::Reconnecting,
            ConnectionState::Connected { .. } => self.connected_receiver_status(),
        }
    }

    pub fn sender_status(self) -> SenderStatus {
        if self.output == OutputState::PermissionRequired {
            return SenderStatus::PermissionRequired;
        }
        match self.connection {
            ConnectionState::Idle => SenderStatus::Disconnected,
            ConnectionState::Listening => SenderStatus::Discovering,
            ConnectionState::Connecting => SenderStatus::Connecting,
            ConnectionState::Reconnecting { .. } => SenderStatus::Reconnecting,
            ConnectionState::Connected { .. } => self.connected_sender_status(),
        }
    }

    fn connected_receiver_status(self) -> ReceiverStatus {
        if self.trust == TrustState::Pairing {
            return ReceiverStatus::Pairing;
        }
        match self.stream {
            StreamState::Negotiating => ReceiverStatus::Negotiating,
            stream if stream.is_streaming() && self.trust == TrustState::Authenticated => {
                if self.health == HealthState::NetworkDegraded {
                    ReceiverStatus::NetworkUnstable
                } else {
                    ReceiverStatus::Streaming
                }
            }
            StreamState::Idle
            | StreamState::Stopping
            | StreamState::AwaitingRefresh { .. }
            | StreamState::Streaming { .. } => ReceiverStatus::Connecting,
        }
    }

    fn connected_sender_status(self) -> SenderStatus {
        if self.trust == TrustState::Pairing {
            return SenderStatus::Pairing;
        }
        match self.stream {
            StreamState::Negotiating => SenderStatus::Negotiating,
            stream if stream.is_streaming() && self.trust == TrustState::Authenticated => {
                if self.health == HealthState::NetworkDegraded {
                    SenderStatus::NetworkUnstable
                } else {
                    SenderStatus::Streaming
                }
            }
            StreamState::Idle
            | StreamState::Stopping
            | StreamState::AwaitingRefresh { .. }
            | StreamState::Streaming { .. } => SenderStatus::Connecting,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_and_output_do_not_destroy_connected_streaming_facts() {
        let mut state = SessionRuntimeState::default();
        state.set_connection(ConnectionState::Connected { generation: 7 });
        state.set_trust(TrustState::Authenticated);
        state.set_stream(StreamState::Streaming { generation: 11 });
        state.set_health(HealthState::NetworkDegraded);
        state.set_output(OutputState::VirtualCameraUnavailable);

        assert_eq!(state.receiver_status(), ReceiverStatus::NetworkUnstable);
        assert_eq!(
            state.connection(),
            ConnectionState::Connected { generation: 7 }
        );
        assert_eq!(state.stream(), StreamState::Streaming { generation: 11 });
        assert_eq!(state.output(), OutputState::VirtualCameraUnavailable);
    }

    #[test]
    fn idle_output_projects_without_replacing_connection() {
        let mut state = SessionRuntimeState::default();
        state.set_connection(ConnectionState::Listening);
        state.set_output(OutputState::VirtualCameraUnavailable);
        assert_eq!(
            state.receiver_status(),
            ReceiverStatus::VirtualCameraUnavailable
        );
        assert_eq!(state.connection(), ConnectionState::Listening);

        state.set_output(OutputState::Ready);
        assert_eq!(state.receiver_status(), ReceiverStatus::Discovering);
    }
}
