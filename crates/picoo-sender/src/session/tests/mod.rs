use super::*;

use std::collections::VecDeque;

use bytes::Bytes;
use picoo_protocol::VideoPacket;
use picoo_transport::{ChannelBinding, PicooTransport, TransportError, TransportEvent};

pub(super) use std::time::Duration;

pub(super) use picoo_protocol::control::control_envelope::Payload as ControlPayload;
pub(super) use picoo_protocol::control::ReceiverStats as ReceiverStatsMsg;
pub(super) use picoo_protocol::control::{Capabilities, PairingChallenge, Resolution, ServerHello};
pub(super) use picoo_rate_control::BitrateAction;
pub(super) use picoo_session::SenderStatus;
pub(super) use picoo_testkit::MemoryTransport;
pub(super) use picoo_transport::{CloseReason, Endpoint, SessionId};

pub(super) use crate::stream_config::StreamConfigParams;
pub(super) use crate::SenderError;

mod abr;
mod epoch;
mod pairing;
mod reconnect;

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

    fn send_video(
        &mut self,
        _session: SessionId,
        _packet: VideoPacket,
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
