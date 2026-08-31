use picoo_protocol::control::{
    CameraCommand, Capabilities, EncoderCommand, PairingApproval, PairingChallenge,
    PairingComplete, ReceiverStats as ReceiverStatsMsg, ServerHello, SessionError,
};
use picoo_transport::{PicooTransport, SessionId};
use prost::Message;

use super::SenderSession;

impl<T: PicooTransport> SenderSession<T> {
    pub(super) fn handle_control(&mut self, session: SessionId, msg: bytes::Bytes) {
        if let Ok(approval) = PairingApproval::decode(msg.as_ref()) {
            if self.handle_pairing_approval(session, &approval) {
                return;
            }
        }
        if let Ok(complete) = PairingComplete::decode(msg.as_ref()) {
            if self.handle_pairing_complete(session, &complete) {
                return;
            }
        }
        if let Ok(stats) = ReceiverStatsMsg::decode(msg.as_ref()) {
            self.apply_receiver_stats(stats);
            return;
        }
        if let Ok(command) = EncoderCommand::decode(msg.as_ref()) {
            if self.handle_encoder_command(&command) {
                return;
            }
        }
        if let Ok(cam) = CameraCommand::decode(msg.as_ref()) {
            if self.handle_camera_command(cam) {
                return;
            }
        }
        if let Ok(capabilities) = Capabilities::decode(msg.as_ref()) {
            if self.handle_capabilities(capabilities) {
                return;
            }
        }
        if let Ok(challenge) = PairingChallenge::decode(msg.as_ref()) {
            if self.handle_pairing_challenge(challenge) {
                return;
            }
        }
        // Known SessionError codes before ServerHello — all use string field 1.
        if let Ok(err) = SessionError::decode(msg.as_ref()) {
            if self.handle_session_error(err) {
                return;
            }
        }
        if let Ok(hello) = ServerHello::decode(msg.as_ref()) {
            self.on_server_hello(hello);
        }
    }
}
