//! Desktop receiver session: QUIC ingress → reassembly → FrameHub.
//!
//! REQ-PICOO-FRAME-001, REQ-PICOO-MEDIA-005/006 (decode placeholder until MF/VT).
//! REQ-PICOO-PAIRING-*: ClientHello/ServerHello gate before video ingress.

use std::time::Duration;

use bytes::Bytes;
use picoo_frame_hub::{FrameHub, FrameSlot};
use picoo_packet::ReassemblyMap;
use picoo_pairing::{PairingError, TrustedDeviceStore};
use picoo_protocol::control::{ClientHello, ServerHello};
use picoo_protocol::ALPN;
use picoo_session::ReceiverStatus;
use picoo_transport::{
    CloseReason, Endpoint, QuicReceiverTransport, SessionId, TransportError, TransportEvent,
};
use prost::Message;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ReceiverError {
    #[error("transport: {0}")]
    Transport(#[from] TransportError),
    #[error("sender: {0}")]
    Sender(#[from] picoo_sender::SenderError),
    #[error("frame hub: {0}")]
    FrameHub(#[from] picoo_frame_hub::FrameHubError),
    #[error("pairing: {0}")]
    Pairing(#[from] PairingError),
    #[error("protocol: {0}")]
    Protocol(String),
    #[error("not listening")]
    NotListening,
    #[error("loopback timeout")]
    LoopbackTimeout,
}

#[derive(Debug, Clone)]
pub struct ReceiverIdentity {
    pub receiver_id: String,
    pub display_name: String,
    pub public_key: Vec<u8>,
}

impl Default for ReceiverIdentity {
    fn default() -> Self {
        Self {
            receiver_id: "windows-receiver".into(),
            display_name: "Picoo Camera".into(),
            public_key: vec![0x04, 0x01],
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReceiverStats {
    pub access_units: u64,
    pub packets_received: u64,
    pub packets_dropped_unpaired: u64,
}

struct ActiveSender {
    sender_id: String,
    #[allow(dead_code)]
    device_name: String,
    #[allow(dead_code)]
    public_key: Vec<u8>,
    video_allowed: bool,
}

pub struct ReceiverSession {
    transport: QuicReceiverTransport,
    reassembly: ReassemblyMap,
    frame_hub: FrameHub,
    identity: ReceiverIdentity,
    trusted: TrustedDeviceStore,
    active_sender: Option<ActiveSender>,
    status: ReceiverStatus,
    stats: ReceiverStats,
    permit_unpaired_video: bool,
}

impl Default for ReceiverSession {
    fn default() -> Self {
        Self::new()
    }
}

impl ReceiverSession {
    pub fn new() -> Self {
        Self {
            transport: QuicReceiverTransport::new(),
            reassembly: ReassemblyMap::new(8, 16),
            frame_hub: FrameHub::new(),
            identity: ReceiverIdentity::default(),
            trusted: TrustedDeviceStore::new(),
            active_sender: None,
            status: ReceiverStatus::Disconnected,
            stats: ReceiverStats::default(),
            permit_unpaired_video: false,
        }
    }

    pub fn with_identity(mut self, identity: ReceiverIdentity) -> Self {
        self.identity = identity;
        self
    }

    pub fn trusted_devices(&self) -> &TrustedDeviceStore {
        &self.trusted
    }

    pub fn trusted_devices_mut(&mut self) -> &mut TrustedDeviceStore {
        &mut self.trusted
    }

    /// Test/loopback helper — production receivers keep the default `false`.
    pub fn set_permit_unpaired_video(&mut self, permit: bool) {
        self.permit_unpaired_video = permit;
    }

    pub fn status(&self) -> ReceiverStatus {
        self.status
    }

    pub fn stats(&self) -> ReceiverStats {
        self.stats
    }

    pub fn is_connected(&self) -> bool {
        self.transport.is_connected()
    }

    pub fn pairing_required(&self) -> bool {
        self.active_sender
            .as_ref()
            .is_some_and(|sender| !sender.video_allowed)
    }

    pub fn active_sender_id(&self) -> Option<&str> {
        self.active_sender
            .as_ref()
            .map(|sender| sender.sender_id.as_str())
    }

    pub fn frame_hub(&self) -> &FrameHub {
        &self.frame_hub
    }

    pub fn bind_addr(&self) -> Option<std::net::SocketAddr> {
        self.transport.bind_addr()
    }

    pub fn listen(&mut self, endpoint: Endpoint) -> Result<std::net::SocketAddr, ReceiverError> {
        let addr = self.transport.bind(endpoint)?;
        self.status = ReceiverStatus::Discovering;
        Ok(addr)
    }

    pub fn pump(&mut self) -> Result<(), ReceiverError> {
        self.transport.pump()?;

        while let Some(event) = self.transport.poll_event() {
            match event {
                TransportEvent::Connected(_) => {
                    self.status = ReceiverStatus::Connecting;
                }
                TransportEvent::Disconnected(_, _) => {
                    self.status = ReceiverStatus::Disconnected;
                    self.active_sender = None;
                    self.reassembly = ReassemblyMap::new(8, 16);
                }
                TransportEvent::ControlMessage(session, msg) => {
                    self.handle_control(session, msg)?;
                }
                TransportEvent::VideoPacket(_, packet) => {
                    self.stats.packets_received += 1;
                    if !self.video_allowed() {
                        self.stats.packets_dropped_unpaired += 1;
                        continue;
                    }
                    if let Some(access_unit) = self.reassembly.ingest(packet).ok().flatten() {
                        self.publish_access_unit(access_unit)?;
                    }
                }
            }
        }

        Ok(())
    }

    fn video_allowed(&self) -> bool {
        if self.permit_unpaired_video {
            return true;
        }
        self.active_sender
            .as_ref()
            .is_some_and(|sender| sender.video_allowed)
    }

    fn handle_control(&mut self, session: SessionId, msg: Bytes) -> Result<(), ReceiverError> {
        let hello = ClientHello::decode(msg.as_ref())
            .map_err(|e| ReceiverError::Protocol(format!("ClientHello decode: {e}")))?;

        let paired = self
            .trusted
            .verify_paired_key(&hello.sender_id, &hello.public_key)
            .is_ok();
        let pairing_required = !paired;

        let server_hello = ServerHello {
            receiver_id: self.identity.receiver_id.clone(),
            display_name: self.identity.display_name.clone(),
            protocol_version: ALPN.into(),
            public_key: self.identity.public_key.clone(),
            pairing_required,
        };
        let mut out = Vec::new();
        server_hello
            .encode(&mut out)
            .map_err(|e| ReceiverError::Protocol(format!("ServerHello encode: {e}")))?;
        self.transport.send_control(session, Bytes::from(out))?;

        self.active_sender = Some(ActiveSender {
            sender_id: hello.sender_id,
            device_name: hello.device_name,
            public_key: hello.public_key,
            video_allowed: paired,
        });
        self.status = if pairing_required {
            ReceiverStatus::Pairing
        } else {
            ReceiverStatus::Streaming
        };

        Ok(())
    }

    pub fn close(&mut self) {
        if self.transport.is_connected() {
            self.transport
                .close(picoo_transport::SessionId(1), CloseReason::LocalClose);
        }
        self.status = ReceiverStatus::Disconnected;
        self.active_sender = None;
    }

    /// Placeholder decode path: store H.264 access unit bytes until MF/VT decoder lands.
    fn publish_access_unit(&mut self, access_unit: Bytes) -> Result<(), ReceiverError> {
        self.stats.access_units += 1;
        let index = self.frame_hub.begin_write()?;
        self.frame_hub
            .commit_write(index, 1280, 720, 1280, 0, 0, access_unit);
        Ok(())
    }

    pub fn latest_frame(&self) -> Option<&FrameSlot> {
        self.frame_hub.latest_ready()
    }
}

/// Run sender→receiver loopback until one access unit reaches FrameHub.
pub fn run_loopback_access_unit(payload: &[u8]) -> Result<Bytes, ReceiverError> {
    use picoo_sender::SenderSession;
    use picoo_transport::{Endpoint, QuicSenderTransport};

    let mut receiver = ReceiverSession::new();
    receiver.set_permit_unpaired_video(true);
    let bind = receiver.listen(Endpoint {
        host: "127.0.0.1".into(),
        port: 0,
    })?;

    let mut sender = SenderSession::new(QuicSenderTransport::new());
    let endpoint = Endpoint {
        host: bind.ip().to_string(),
        port: bind.port(),
    };
    sender.connect(endpoint)?;

    for _ in 0..500 {
        receiver.pump()?;
        sender.pump()?;
        if sender.is_connected() && receiver.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    if !receiver.is_connected() {
        return Err(ReceiverError::LoopbackTimeout);
    }

    sender.ingest_and_flush(payload, true, 1, 1)?;

    for _ in 0..200 {
        receiver.pump()?;
        sender.pump().ok();
        if let Some(frame) = receiver.latest_frame() {
            return Ok(frame.pixel_data.clone());
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    Err(ReceiverError::LoopbackTimeout)
}

#[cfg(test)]
mod tests;
