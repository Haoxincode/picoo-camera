use picoo_protocol::control::control_envelope::Payload as ControlPayload;
use picoo_session::{ConnectionState, StreamState, TrustState};
use picoo_transport::{PicooTransport, SessionId};

use super::SenderSession;

impl<T: PicooTransport> SenderSession<T> {
    pub(super) fn handle_control(&mut self, session: SessionId, msg: bytes::Bytes) {
        if self.active_session() != Some(session) {
            return;
        }
        let Ok(envelope) = picoo_protocol::decode_control_envelope(&msg) else {
            self.reject_authentication("INVALID_CONTROL_ENVELOPE");
            return;
        };
        if envelope.connection_generation != session.0
            || envelope.message_id <= self.last_received_control_message_id
        {
            self.reject_authentication("STALE_CONTROL_ENVELOPE");
            return;
        }
        self.last_received_control_message_id = envelope.message_id;
        let payload = envelope.payload.expect("validated envelope payload");
        if !self.control_payload_allowed(&payload) {
            self.reject_authentication("CONTROL_PAYLOAD_NOT_ALLOWED");
            return;
        }
        match payload {
            ControlPayload::PairingApproval(approval) => {
                if !self.handle_pairing_approval(session, &approval) {
                    self.reject_authentication("INVALID_RECEIVER_APPROVAL");
                }
            }
            ControlPayload::PairingComplete(complete) => {
                if !self.handle_pairing_complete(session, &complete) {
                    self.reject_authentication("INVALID_RECEIVER_COMPLETE");
                }
            }
            ControlPayload::ReceiverStats(stats) => self.apply_receiver_stats(stats),
            ControlPayload::EncoderCommand(command) => {
                self.handle_encoder_command(&command);
            }
            ControlPayload::CameraCommand(command) => {
                self.handle_camera_command(command);
            }
            ControlPayload::Capabilities(capabilities) => {
                self.handle_capabilities(capabilities);
            }
            ControlPayload::SessionError(error) => {
                self.handle_session_error(error);
            }
            ControlPayload::ServerHello(hello) => self.on_server_hello(hello),
            ControlPayload::ClockSyncPing(ping) => self.handle_clock_sync_ping(session, ping),
            _ => {}
        }
    }

    fn control_payload_allowed(&self, payload: &ControlPayload) -> bool {
        match payload {
            ControlPayload::ServerHello(_) => {
                matches!(
                    self.lifecycle.runtime.connection(),
                    ConnectionState::Connected { .. }
                ) && self.lifecycle.runtime.stream() == StreamState::Negotiating
                    && self.lifecycle.runtime.trust() == TrustState::Unknown
            }
            ControlPayload::PairingApproval(_) | ControlPayload::PairingComplete(_) => {
                self.lifecycle.runtime.stream() == StreamState::Negotiating
                    && matches!(
                        self.lifecycle.runtime.trust(),
                        TrustState::Unknown | TrustState::Pairing
                    )
            }
            ControlPayload::Capabilities(_)
            | ControlPayload::ReceiverStats(_)
            | ControlPayload::EncoderCommand(_)
            | ControlPayload::CameraCommand(_)
            | ControlPayload::ClockSyncPing(_) => {
                self.lifecycle.runtime.stream().is_streaming()
                    && self.lifecycle.runtime.trust() == TrustState::Authenticated
                    && self.receiver_is_authenticated()
            }
            ControlPayload::SessionError(_) => true,
            _ => false,
        }
    }
}
