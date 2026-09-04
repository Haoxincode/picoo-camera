use super::*;

use std::collections::VecDeque;

use bytes::Bytes;
use picoo_transport::{
    ChannelBinding, PicooTransport, TransportError, TransportEvent, VideoDatagramBatch,
};

pub(super) use std::time::Duration;

pub(super) use picoo_protocol::control::control_envelope::Payload as ControlPayload;
pub(super) use picoo_protocol::control::ReceiverStats as ReceiverStatsMsg;
pub(super) use picoo_protocol::control::{Capabilities, Resolution, ServerHello};
pub(super) use picoo_rate_control::BitrateAction;
pub(super) use picoo_session::SenderStatus;
pub(super) use picoo_transport::{CloseReason, Endpoint, SessionId};

pub(super) use crate::stream_config::StreamConfigParams;
pub(super) use crate::SenderError;

fn native_au(
    data: &[u8],
    is_keyframe: bool,
    pts_us: u64,
    facts: (u64, u64, u32, u32),
) -> crate::NativeEncoderAccessUnit<'_> {
    crate::NativeEncoderAccessUnit {
        data,
        is_keyframe,
        pts_us,
        transaction_id: facts.0,
        encoder_generation: facts.1,
        stream_epoch: facts.2,
        height: facts.3,
    }
}

mod abr;
mod epoch;
mod pairing;
mod reconnect;

struct MemoryTransport {
    next_session: u64,
    connected: Option<SessionId>,
    events: VecDeque<TransportEvent>,
}

impl MemoryTransport {
    fn new() -> Self {
        Self {
            next_session: 1,
            connected: None,
            events: VecDeque::new(),
        }
    }
}

impl PicooTransport for MemoryTransport {
    fn connect(&mut self, _endpoint: Endpoint) -> Result<SessionId, TransportError> {
        let session = SessionId(self.next_session);
        self.next_session += 1;
        self.connected = Some(session);
        self.events.push_back(TransportEvent::Connected(session));
        Ok(session)
    }

    fn send_control(&mut self, session: SessionId, _message: Bytes) -> Result<(), TransportError> {
        if self.connected == Some(session) {
            Ok(())
        } else {
            Err(TransportError::NotConnected)
        }
    }

    fn send_video_batch(
        &mut self,
        session: SessionId,
        _batch: VideoDatagramBatch,
    ) -> Result<(), TransportError> {
        if self.connected == Some(session) {
            Ok(())
        } else {
            Err(TransportError::NotConnected)
        }
    }

    fn poll_event(&mut self) -> Option<TransportEvent> {
        self.events.pop_front()
    }

    fn close(&mut self, session: SessionId, reason: CloseReason) {
        if self.connected == Some(session) {
            self.connected = None;
        }
        self.events
            .push_back(TransportEvent::Disconnected(session, reason));
    }

    fn channel_binding(&self, session: SessionId) -> Result<ChannelBinding, TransportError> {
        if self.connected == Some(session) {
            Ok([0x42; 32])
        } else {
            Err(TransportError::ChannelBindingUnavailable)
        }
    }
}

fn signed_server_hello<T: PicooTransport>(
    sender: &SenderSession<T>,
    receiver: &picoo_pairing::DeviceIdentity,
    pairing_required: bool,
) -> (ServerHello, [u8; 32]) {
    let session = sender.active_session().expect("connected Sender");
    let sender_nonce = sender.sender_nonce.expect("ClientHello nonce");
    let receiver_nonce = [0x24; 32];
    let channel_binding = sender
        .transport
        .channel_binding(session)
        .expect("channel binding");
    let transcript = picoo_pairing::PairingTranscript {
        sender_id: sender.identity.device_id(),
        sender_public_key: sender.identity.public_key(),
        sender_nonce: &sender_nonce,
        receiver_id: receiver.device_id(),
        receiver_public_key: receiver.public_key(),
        receiver_nonce: &receiver_nonce,
        channel_binding: &channel_binding,
        connection_generation: session.0,
    };
    let transcript_hash = transcript.hash().expect("transcript");
    (
        ServerHello {
            receiver_id: receiver.device_id().to_owned(),
            display_name: receiver.device_name().to_owned(),
            public_key: receiver.public_key().to_vec(),
            pairing_required,
            receiver_nonce: receiver_nonce.to_vec(),
            transcript_hash: transcript_hash.to_vec(),
            identity_signature: picoo_pairing::sign_transcript_phase(
                receiver,
                &transcript_hash,
                super::pairing::SERVER_HELLO_PHASE,
            )
            .to_vec(),
        },
        transcript_hash,
    )
}

fn authenticate_trusted_receiver<T: PicooTransport>(
    sender: &mut SenderSession<T>,
    receiver: &picoo_pairing::DeviceIdentity,
) {
    sender
        .trusted_devices_mut()
        .upsert(picoo_pairing::trusted_device_from_pairing(
            receiver.device_id(),
            receiver.device_name(),
            receiver.public_key(),
            1,
        ));
    let (hello, transcript_hash) = signed_server_hello(sender, receiver, false);
    sender
        .inject_control_payload_for_test(ControlPayload::ServerHello(hello))
        .expect("authenticated ServerHello");
    let approval = picoo_protocol::control::PairingApproval {
        transcript_hash: transcript_hash.to_vec(),
        identity_signature: picoo_pairing::sign_transcript_phase(
            receiver,
            &transcript_hash,
            super::pairing::PAIRING_APPROVAL_PHASE,
        )
        .to_vec(),
    };
    sender
        .inject_control_payload_for_test(ControlPayload::PairingApproval(approval))
        .expect("Receiver approval");
    let complete = picoo_protocol::control::PairingComplete {
        transcript_hash: transcript_hash.to_vec(),
        identity_signature: picoo_pairing::sign_transcript_phase(
            receiver,
            &transcript_hash,
            super::pairing::PAIRING_COMPLETE_PHASE,
        )
        .to_vec(),
    };
    sender
        .inject_control_payload_for_test(ControlPayload::PairingComplete(complete))
        .expect("Receiver completion");
}

struct DeferredConnectTransport {
    session: SessionId,
    connected: bool,
    events: VecDeque<TransportEvent>,
    sent_control: Vec<Bytes>,
}

impl DeferredConnectTransport {
    fn new() -> Self {
        Self {
            session: SessionId(1),
            connected: false,
            events: VecDeque::new(),
            sent_control: Vec::new(),
        }
    }

    fn complete_connect(&mut self) {
        self.connected = true;
        self.events
            .push_back(TransportEvent::Connected(self.session));
    }
}

impl PicooTransport for DeferredConnectTransport {
    fn connect(&mut self, _endpoint: Endpoint) -> Result<SessionId, TransportError> {
        Ok(self.session)
    }

    fn send_control(&mut self, session: SessionId, message: Bytes) -> Result<(), TransportError> {
        if !self.connected || session != self.session {
            return Err(TransportError::NotConnected);
        }
        self.sent_control.push(message);
        Ok(())
    }

    fn send_video_batch(
        &mut self,
        _session: SessionId,
        _batch: VideoDatagramBatch,
    ) -> Result<(), TransportError> {
        Ok(())
    }

    fn poll_event(&mut self) -> Option<TransportEvent> {
        self.events.pop_front()
    }

    fn close(&mut self, session: SessionId, reason: CloseReason) {
        self.connected = false;
        self.events
            .push_back(TransportEvent::Disconnected(session, reason));
    }

    fn channel_binding(&self, session: SessionId) -> Result<ChannelBinding, TransportError> {
        if self.connected && session == self.session {
            Ok([0x42; 32])
        } else {
            Err(TransportError::ChannelBindingUnavailable)
        }
    }
}

struct RejectConnectTransport;

impl PicooTransport for RejectConnectTransport {
    fn connect(&mut self, _endpoint: Endpoint) -> Result<SessionId, TransportError> {
        Err(TransportError::NetworkBindingFailed(
            "platform rejected Wi-Fi socket binding".into(),
        ))
    }

    fn send_control(&mut self, _session: SessionId, _message: Bytes) -> Result<(), TransportError> {
        Err(TransportError::NotConnected)
    }

    fn send_video_batch(
        &mut self,
        _session: SessionId,
        _batch: VideoDatagramBatch,
    ) -> Result<(), TransportError> {
        Err(TransportError::NotConnected)
    }

    fn poll_event(&mut self) -> Option<TransportEvent> {
        None
    }

    fn close(&mut self, _session: SessionId, _reason: CloseReason) {}

    fn channel_binding(&self, _session: SessionId) -> Result<ChannelBinding, TransportError> {
        Err(TransportError::ChannelBindingUnavailable)
    }
}
