//! Pure Receiver control-plane phase whitelist — REQ-PICOO-PROTOCOL-012/013.

use crate::control::control_envelope::Payload;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverControlPhase {
    AwaitingClientHello,
    Pairing,
    AuthenticatedIdle,
    Streaming,
}

pub fn receiver_payload_allowed(phase: ReceiverControlPhase, payload: &Payload) -> bool {
    match phase {
        ReceiverControlPhase::AwaitingClientHello => matches!(payload, Payload::ClientHello(_)),
        ReceiverControlPhase::Pairing => matches!(
            payload,
            Payload::PairingCommit(_)
                | Payload::PairingConfirm(_)
                | Payload::StartStream(_)
                | Payload::StopStream(_)
        ),
        ReceiverControlPhase::AuthenticatedIdle => matches!(
            payload,
            Payload::StartStream(_)
                | Payload::StopStream(_)
                | Payload::SenderStats(_)
                | Payload::StreamConfig(_)
        ),
        ReceiverControlPhase::Streaming => matches!(
            payload,
            Payload::StartStream(_)
                | Payload::StopStream(_)
                | Payload::SenderStats(_)
                | Payload::StreamConfig(_)
                | Payload::ClockSyncPong(_)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::{CameraCommand, ClientHello, StreamConfig};

    #[test]
    fn unauthenticated_phases_never_accept_media_or_privileged_configuration() {
        let config = Payload::StreamConfig(StreamConfig::default());
        let camera = Payload::CameraCommand(CameraCommand::default());
        for phase in [
            ReceiverControlPhase::AwaitingClientHello,
            ReceiverControlPhase::Pairing,
        ] {
            assert!(!receiver_payload_allowed(phase, &config));
            assert!(!receiver_payload_allowed(phase, &camera));
        }
        assert!(receiver_payload_allowed(
            ReceiverControlPhase::AwaitingClientHello,
            &Payload::ClientHello(ClientHello::default())
        ));
    }

    #[test]
    fn clock_response_requires_streaming_but_stream_config_can_negotiate_idle() {
        let pong = Payload::ClockSyncPong(crate::control::ClockSyncPong::default());
        let config = Payload::StreamConfig(StreamConfig::default());
        assert!(!receiver_payload_allowed(
            ReceiverControlPhase::AuthenticatedIdle,
            &pong
        ));
        assert!(receiver_payload_allowed(
            ReceiverControlPhase::Streaming,
            &pong
        ));
        assert!(receiver_payload_allowed(
            ReceiverControlPhase::AuthenticatedIdle,
            &config
        ));
    }
}
