use picoo_protocol::control::control_envelope::Payload as ControlPayload;
use picoo_session::SenderStatus;
use picoo_transport::{PicooTransport, SessionId};

use super::SenderSession;

impl<T: PicooTransport> SenderSession<T> {
    pub(super) fn handle_control(&mut self, session: SessionId, msg: bytes::Bytes) {
        if self.session != Some(session) {
            return;
        }
        let Ok(envelope) = picoo_protocol::decode_control_envelope(&msg) else {
            self.last_session_error = Some("INVALID_CONTROL_ENVELOPE".into());
            return;
        };
        if envelope.connection_generation != session.0
            || envelope.message_id <= self.last_received_control_message_id
        {
            self.last_session_error = Some("STALE_CONTROL_ENVELOPE".into());
            return;
        }
        self.last_received_control_message_id = envelope.message_id;
        let payload = envelope.payload.expect("validated envelope payload");
        if !self.control_payload_allowed(&payload) {
            self.last_session_error = Some("CONTROL_PAYLOAD_NOT_ALLOWED".into());
            return;
        }
        match payload {
            ControlPayload::PairingApproval(approval) => {
                self.handle_pairing_approval(session, &approval);
            }
            ControlPayload::PairingComplete(complete) => {
                self.handle_pairing_complete(session, &complete);
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
            ControlPayload::PairingChallenge(challenge) => {
                self.handle_pairing_challenge(challenge);
            }
            ControlPayload::SessionError(error) => {
                self.handle_session_error(error);
            }
            ControlPayload::ServerHello(hello) => self.on_server_hello(hello),
            _ => {}
        }
    }

    fn control_payload_allowed(&self, payload: &ControlPayload) -> bool {
        match payload {
            ControlPayload::ServerHello(_) => self.status == SenderStatus::Negotiating,
            ControlPayload::PairingChallenge(_)
            | ControlPayload::PairingApproval(_)
            | ControlPayload::PairingComplete(_) => self.status == SenderStatus::Pairing,
            ControlPayload::Capabilities(_)
            | ControlPayload::ReceiverStats(_)
            | ControlPayload::EncoderCommand(_)
            | ControlPayload::CameraCommand(_) => {
                matches!(
                    self.status,
                    SenderStatus::Streaming | SenderStatus::NetworkUnstable
                ) && self.receiver_is_authenticated()
            }
            ControlPayload::SessionError(_) => true,
            _ => false,
        }
    }
}
