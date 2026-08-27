//! Sender session: packetization + transport flush + reconnect + bitrate control.
//!
//! REQ-PICOO-SESSION-001, REQ-PICOO-TRANSPORT-004, REQ-PICOO-MEDIA-007

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use picoo_metrics::ReceiverStats as MetricsReceiverStats;
use picoo_pairing::{pairing_confirm_signature, trusted_device_from_pairing, TrustedDeviceStore};
use picoo_protocol::control::{
    Capabilities, ClientHello, PairingChallenge, PairingConfirm, ReceiverStats as ReceiverStatsMsg,
    ServerHello,
};
use picoo_protocol::VideoPacket;
use picoo_protocol::ALPN;
use picoo_rate_control::{BitrateAction, BitrateController};
use picoo_session::{ReconnectBackoff, SenderStatus};
use picoo_transport::{Endpoint, PicooTransport, SessionId, TransportEvent};
use prost::Message;

use crate::stream_config::StreamConfigParams;
use crate::{SenderError, SenderPipeline, SenderStats};

const DEFAULT_INITIAL_BITRATE_BPS: u32 = 6_000_000;
const DEFAULT_MIN_BITRATE_BPS: u32 = 3_000_000;
const DEFAULT_MAX_BITRATE_BPS: u32 = 10_000_000;

#[derive(Debug, Clone)]
struct SenderPairing {
    receiver_id: String,
    display_name: String,
    public_key: Vec<u8>,
    challenge_nonce: Vec<u8>,
    short_code: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionStats {
    pub pipeline: SenderStats,
    pub sent_datagrams: u64,
}

pub struct SenderSession<T: PicooTransport> {
    pipeline: SenderPipeline,
    transport: T,
    session: Option<SessionId>,
    sent_datagrams: u64,
    pairing: Option<SenderPairing>,
    sender_id: Option<String>,
    hello_params: Option<(String, String, Vec<u8>)>,
    trusted: TrustedDeviceStore,
    trusted_store_path: Option<PathBuf>,
    status: SenderStatus,
    last_endpoint: Option<Endpoint>,
    reconnect_backoff: ReconnectBackoff,
    reconnect_after: Option<Instant>,
    auto_reconnect: bool,
    bitrate: BitrateController,
    last_bitrate_action: BitrateAction,
    last_receiver_stats: Option<MetricsReceiverStats>,
    pending_stream_config: Option<StreamConfigParams>,
    receiver_capabilities: Option<Capabilities>,
    stream_config_sent: bool,
}

impl<T: PicooTransport> SenderSession<T> {
    pub fn new(transport: T) -> Self {
        Self {
            pipeline: SenderPipeline::default(),
            transport,
            session: None,
            sent_datagrams: 0,
            pairing: None,
            sender_id: None,
            hello_params: None,
            trusted: TrustedDeviceStore::new(),
            trusted_store_path: None,
            status: SenderStatus::Disconnected,
            last_endpoint: None,
            reconnect_backoff: ReconnectBackoff::default(),
            reconnect_after: None,
            auto_reconnect: true,
            bitrate: BitrateController::new(
                DEFAULT_INITIAL_BITRATE_BPS,
                DEFAULT_MIN_BITRATE_BPS,
                DEFAULT_MAX_BITRATE_BPS,
            ),
            last_bitrate_action: BitrateAction::Hold,
            last_receiver_stats: None,
            pending_stream_config: Some(StreamConfigParams::default()),
            receiver_capabilities: None,
            stream_config_sent: false,
        }
    }

    pub fn status(&self) -> SenderStatus {
        self.status
    }

    pub fn set_auto_reconnect(&mut self, enabled: bool) {
        self.auto_reconnect = enabled;
    }

    pub fn current_bitrate_bps(&self) -> u32 {
        self.bitrate.current_bitrate_bps()
    }

    pub fn last_bitrate_action(&self) -> BitrateAction {
        self.last_bitrate_action
    }

    pub fn last_receiver_stats(&self) -> Option<&MetricsReceiverStats> {
        self.last_receiver_stats.as_ref()
    }

    pub fn receiver_capabilities(&self) -> Option<&Capabilities> {
        self.receiver_capabilities.as_ref()
    }

    pub fn set_stream_config(&mut self, config: StreamConfigParams) {
        self.pending_stream_config = Some(config);
    }

    pub fn with_trusted_store(mut self, path: impl AsRef<Path>) -> Result<Self, SenderError> {
        let path = path.as_ref().to_path_buf();
        self.trusted = TrustedDeviceStore::load_from_path(&path)?;
        self.trusted_store_path = Some(path);
        Ok(self)
    }

    pub fn attach_trusted_store(&mut self, path: impl AsRef<Path>) -> Result<(), SenderError> {
        let path = path.as_ref().to_path_buf();
        self.trusted = TrustedDeviceStore::load_from_path(&path)?;
        self.trusted_store_path = Some(path);
        Ok(())
    }

    pub fn trusted_devices(&self) -> &TrustedDeviceStore {
        &self.trusted
    }

    pub fn remove_trusted_device(&mut self, device_id: &str) -> Result<bool, SenderError> {
        let removed = self.trusted.remove(device_id);
        if removed {
            self.persist_trusted()?;
        }
        Ok(removed)
    }

    pub fn connected_receiver_id(&self) -> Option<&str> {
        self.pairing
            .as_ref()
            .and_then(|p| (!p.receiver_id.is_empty()).then_some(p.receiver_id.as_str()))
    }

    fn persist_trusted(&self) -> Result<(), SenderError> {
        if let Some(path) = &self.trusted_store_path {
            self.trusted.save_to_path(path)?;
        }
        Ok(())
    }

    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    fn enter_streaming(&mut self) {
        self.status = SenderStatus::Streaming;
        let _ = self.send_pending_stream_config();
    }

    fn send_pending_stream_config(&mut self) -> Result<(), SenderError> {
        if self.stream_config_sent {
            return Ok(());
        }
        let Some(config) = self.pending_stream_config.clone() else {
            return Ok(());
        };
        self.send_stream_config(&config)?;
        self.stream_config_sent = true;
        Ok(())
    }

    pub fn send_stream_config(&mut self, config: &StreamConfigParams) -> Result<(), SenderError> {
        let session = self.session.ok_or(SenderError::NotConnected)?;
        let msg = config.to_proto();
        let mut buf = Vec::new();
        msg.encode(&mut buf)
            .map_err(|e| SenderError::Protocol(e.to_string()))?;
        self.transport
            .send_control(session, bytes::Bytes::from(buf))
            .map_err(SenderError::Transport)?;
        self.transport.pump().map_err(SenderError::Transport)?;
        self.pending_stream_config = Some(config.clone());
        Ok(())
    }

    fn schedule_reconnect(&mut self) {
        if !self.auto_reconnect || self.last_endpoint.is_none() {
            self.status = SenderStatus::Disconnected;
            return;
        }
        let delay_ms = self.reconnect_backoff.next_delay_ms();
        self.reconnect_after = Some(Instant::now() + Duration::from_millis(delay_ms));
        self.status = SenderStatus::Reconnecting;
    }

    fn try_reconnect(&mut self) -> Result<(), SenderError> {
        let Some(deadline) = self.reconnect_after else {
            return Ok(());
        };
        if Instant::now() < deadline {
            return Ok(());
        }
        self.reconnect_after = None;
        let endpoint = self
            .last_endpoint
            .clone()
            .ok_or(SenderError::NotConnected)?;
        let _ = self.connect(endpoint)?;
        Ok(())
    }

    fn on_connected(&mut self) {
        self.reconnect_backoff.reset();
        self.reconnect_after = None;
        self.status = SenderStatus::Connecting;
        if let Some((sender_id, device_name, public_key)) = self.hello_params.clone() {
            if self
                .send_client_hello(&sender_id, &device_name, &public_key)
                .is_ok()
            {
                self.status = SenderStatus::Negotiating;
            }
        }
    }

    fn drain_events(&mut self) {
        while let Some(event) = self.transport.poll_event() {
            match event {
                TransportEvent::Connected(session) => {
                    self.session = Some(session);
                    self.on_connected();
                }
                TransportEvent::ControlMessage(_, msg) => self.handle_control(msg),
                TransportEvent::Disconnected(_, _) => {
                    self.session = None;
                    self.pairing = None;
                    self.stream_config_sent = false;
                    self.schedule_reconnect();
                }
                TransportEvent::VideoPacket(_, _) => {}
            }
        }
    }

    fn handle_control(&mut self, msg: bytes::Bytes) {
        if let Ok(stats) = ReceiverStatsMsg::decode(msg.as_ref()) {
            let metrics = MetricsReceiverStats {
                rtt_ms: stats.rtt_ms,
                packet_loss: stats.packet_loss,
                jitter_ms: stats.jitter_ms,
                reassembly_drop: stats.reassembly_drop,
                decoder_drop: stats.decoder_drop,
                frame_age_ms: stats.frame_age_ms,
                receive_bitrate: stats.receive_bitrate,
                jitter_buffer_depth_ms: stats.jitter_buffer_depth_ms,
            };
            self.last_receiver_stats = Some(metrics.clone());
            self.last_bitrate_action = self.bitrate.update(&metrics);
            return;
        }
        if let Ok(capabilities) = Capabilities::decode(msg.as_ref()) {
            self.receiver_capabilities = Some(capabilities);
            if self.status == SenderStatus::Negotiating {
                self.enter_streaming();
            }
            return;
        }
        if let Ok(challenge) = PairingChallenge::decode(msg.as_ref()) {
            if let Some(pairing) = self.pairing.as_mut() {
                pairing.challenge_nonce = challenge.challenge_nonce;
                pairing.short_code = challenge.short_code;
            } else {
                self.pairing = Some(SenderPairing {
                    receiver_id: String::new(),
                    display_name: String::new(),
                    public_key: Vec::new(),
                    challenge_nonce: challenge.challenge_nonce,
                    short_code: challenge.short_code,
                });
            }
            self.status = SenderStatus::Pairing;
            return;
        }
        if let Ok(hello) = ServerHello::decode(msg.as_ref()) {
            if self.trusted.is_paired(&hello.receiver_id) {
                if self
                    .trusted
                    .verify_paired_key(&hello.receiver_id, &hello.public_key)
                    .is_err()
                {
                    if let Some(session) = self.session.take() {
                        self.transport
                            .close(session, picoo_transport::CloseReason::LocalClose);
                    }
                    self.status = SenderStatus::Disconnected;
                    self.pairing = None;
                    return;
                }
                self.trusted
                    .touch_last_connected(&hello.receiver_id, self.now_ms());
                let _ = self.persist_trusted();
            }

            if let Some(pairing) = self.pairing.as_mut() {
                pairing.receiver_id = hello.receiver_id.clone();
                pairing.display_name = hello.display_name.clone();
                pairing.public_key = hello.public_key.clone();
            } else if hello.pairing_required {
                self.pairing = Some(SenderPairing {
                    receiver_id: hello.receiver_id,
                    display_name: hello.display_name,
                    public_key: hello.public_key,
                    challenge_nonce: Vec::new(),
                    short_code: String::new(),
                });
                self.status = SenderStatus::Pairing;
            } else {
                self.enter_streaming();
            }
        }
    }

    pub fn stats(&self) -> SessionStats {
        SessionStats {
            pipeline: self.pipeline.stats(),
            sent_datagrams: self.sent_datagrams,
        }
    }

    pub fn is_connected(&self) -> bool {
        self.session.is_some()
    }

    pub fn connect(&mut self, endpoint: Endpoint) -> Result<SessionId, SenderError> {
        self.last_endpoint = Some(endpoint.clone());
        self.status = SenderStatus::Connecting;
        let session = self
            .transport
            .connect(endpoint)
            .map_err(SenderError::Transport)?;
        self.drain_events();
        Ok(session)
    }

    pub fn pump(&mut self) -> Result<(), SenderError> {
        self.transport.pump().map_err(SenderError::Transport)?;
        self.drain_events();
        if self.status == SenderStatus::Reconnecting {
            self.try_reconnect()?;
            self.transport.pump().map_err(SenderError::Transport)?;
            self.drain_events();
        }
        if self.status == SenderStatus::Streaming {
            let _ = self.send_pending_stream_config();
        }
        Ok(())
    }

    pub fn pairing_short_code(&self) -> Option<&str> {
        self.pairing
            .as_ref()
            .and_then(|p| (!p.short_code.is_empty()).then_some(p.short_code.as_str()))
    }

    pub fn ingest_access_unit(
        &mut self,
        data: &[u8],
        is_keyframe: bool,
        pts_us: u64,
        stream_epoch: u32,
    ) -> Result<usize, SenderError> {
        self.pipeline
            .ingest_access_unit(data, is_keyframe, pts_us, stream_epoch)
    }

    /// Send all pending VideoPackets over QUIC datagrams.
    pub fn flush_pending(&mut self) -> Result<usize, SenderError> {
        let session = self.session.ok_or(SenderError::NotConnected)?;
        let packets: Vec<VideoPacket> = self.pipeline.take_pending_packets();
        let mut sent = 0usize;
        for packet in packets {
            self.transport
                .send_video(session, packet)
                .map_err(SenderError::Transport)?;
            sent += 1;
        }
        self.transport.pump().map_err(SenderError::Transport)?;
        self.sent_datagrams += sent as u64;
        Ok(sent)
    }

    pub fn ingest_and_flush(
        &mut self,
        data: &[u8],
        is_keyframe: bool,
        pts_us: u64,
        stream_epoch: u32,
    ) -> Result<usize, SenderError> {
        self.ingest_access_unit(data, is_keyframe, pts_us, stream_epoch)?;
        self.flush_pending()
    }

    pub fn pending_packets(&self) -> usize {
        self.pipeline.pending_packets().len()
    }

    pub fn send_client_hello(
        &mut self,
        sender_id: &str,
        device_name: &str,
        public_key: &[u8],
    ) -> Result<(), SenderError> {
        let session = self.session.ok_or(SenderError::NotConnected)?;
        let hello = ClientHello {
            sender_id: sender_id.into(),
            device_name: device_name.into(),
            protocol_version: ALPN.into(),
            public_key: public_key.to_vec(),
        };
        self.sender_id = Some(sender_id.into());
        self.hello_params = Some((
            sender_id.to_string(),
            device_name.to_string(),
            public_key.to_vec(),
        ));
        let mut buf = Vec::new();
        hello
            .encode(&mut buf)
            .map_err(|e| SenderError::Protocol(e.to_string()))?;
        self.transport
            .send_control(session, bytes::Bytes::from(buf))
            .map_err(SenderError::Transport)?;
        self.transport.pump().map_err(SenderError::Transport)?;
        self.drain_events();
        Ok(())
    }

    pub fn send_pairing_confirm(&mut self, receiver_id: &str) -> Result<(), SenderError> {
        let session = self.session.ok_or(SenderError::NotConnected)?;
        let pairing = self
            .pairing
            .as_ref()
            .ok_or_else(|| SenderError::Protocol("no pairing challenge".into()))?;
        let sender_id = self
            .sender_id
            .as_deref()
            .ok_or_else(|| SenderError::Protocol("missing sender id".into()))?;

        let confirm = PairingConfirm {
            confirm_signature: pairing_confirm_signature(
                &pairing.challenge_nonce,
                receiver_id,
                sender_id,
            ),
        };
        let mut buf = Vec::new();
        confirm
            .encode(&mut buf)
            .map_err(|e| SenderError::Protocol(e.to_string()))?;
        self.transport
            .send_control(session, bytes::Bytes::from(buf))
            .map_err(SenderError::Transport)?;
        self.transport.pump().map_err(SenderError::Transport)?;

        if !receiver_id.is_empty() {
            let display_name = if pairing.display_name.is_empty() {
                receiver_id
            } else {
                pairing.display_name.as_str()
            };
            let now_ms = self.now_ms();
            self.trusted.upsert(trusted_device_from_pairing(
                receiver_id,
                display_name,
                if pairing.public_key.is_empty() {
                    &[]
                } else {
                    &pairing.public_key
                },
                now_ms,
            ));
            self.persist_trusted()?;
        }

        self.status = SenderStatus::Streaming;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn inject_control_for_test(&mut self, msg: bytes::Bytes) -> Result<(), SenderError> {
        self.handle_control(msg);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn disconnect_for_test(&mut self, reason: picoo_transport::CloseReason) {
        if let Some(session) = self.session {
            self.transport.close(session, reason);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use picoo_protocol::control::ReceiverStats as ReceiverStatsMsg;
    use picoo_protocol::control::{PairingChallenge, ServerHello};
    use picoo_protocol::ALPN;
    use picoo_rate_control::BitrateAction;
    use picoo_session::SenderStatus;
    use picoo_testkit::MemoryTransport;
    use picoo_transport::{CloseReason, Endpoint};
    use prost::Message;

    use super::*;

    #[test]
    fn memory_transport_flush_pending() {
        let mut session = SenderSession::new(MemoryTransport::new());
        session
            .connect(Endpoint {
                host: "127.0.0.1".into(),
                port: 1,
            })
            .expect("connect");
        session
            .ingest_access_unit(b"au-bytes", true, 1, 1)
            .expect("ingest");
        let sent = session.flush_pending().expect("flush");
        assert_eq!(sent, 1);
        assert_eq!(session.stats().sent_datagrams, 1);
    }

    #[test]
    fn reconnects_after_disconnect_with_backoff() {
        let mut session = SenderSession::new(MemoryTransport::new());
        let endpoint = Endpoint {
            host: "127.0.0.1".into(),
            port: 4433,
        };
        let _first = session.connect(endpoint.clone()).expect("connect");
        assert!(session.is_connected());

        session.disconnect_for_test(CloseReason::PeerClose);
        session.pump().expect("pump after disconnect");
        assert_eq!(session.status(), SenderStatus::Reconnecting);

        for _ in 0..20 {
            session.pump().expect("reconnect pump");
            if session.is_connected() {
                break;
            }
            std::thread::sleep(Duration::from_millis(600));
        }
        assert!(session.is_connected());
        assert_ne!(session.status(), SenderStatus::Disconnected);
    }

    #[test]
    fn receiver_stats_adjusts_bitrate() {
        let mut session = SenderSession::new(MemoryTransport::new());
        let endpoint = Endpoint {
            host: "127.0.0.1".into(),
            port: 4433,
        };
        session.connect(endpoint).expect("connect");

        let stats = ReceiverStatsMsg {
            packet_loss: 0.05,
            ..Default::default()
        };
        let mut buf = Vec::new();
        stats.encode(&mut buf).expect("encode");
        session
            .inject_control_for_test(bytes::Bytes::from(buf))
            .expect("inject stats");
        session.pump().expect("pump");
        assert_eq!(session.last_bitrate_action(), BitrateAction::Decrease);
        assert!(session.current_bitrate_bps() < DEFAULT_INITIAL_BITRATE_BPS);
    }

    #[test]
    fn resends_client_hello_after_reconnect() {
        let mut session = SenderSession::new(MemoryTransport::new());
        let endpoint = Endpoint {
            host: "127.0.0.1".into(),
            port: 4433,
        };
        session.connect(endpoint.clone()).expect("connect");
        session
            .send_client_hello("phone-1", "Pixel", &[1, 2, 3])
            .expect("hello");

        session.disconnect_for_test(CloseReason::Timeout);
        session.pump().expect("disconnect pump");

        for _ in 0..20 {
            session.pump().expect("reconnect pump");
            if session.is_connected() {
                break;
            }
            std::thread::sleep(Duration::from_millis(600));
        }
        assert!(session.is_connected());
    }

    #[test]
    fn pairing_confirm_persists_trusted_receiver() {
        use picoo_pairing::TrustedDeviceStore;

        let dir = tempfile::tempdir().expect("tempdir");
        let store_path = dir.path().join("trusted.json");

        let mut session = SenderSession::new(MemoryTransport::new())
            .with_trusted_store(&store_path)
            .expect("attach store");
        let endpoint = Endpoint {
            host: "127.0.0.1".into(),
            port: 4433,
        };
        session.connect(endpoint).expect("connect");
        session
            .send_client_hello("android-sender", "Pixel", &[1, 2, 3])
            .expect("client hello");

        let hello = ServerHello {
            receiver_id: "windows-receiver".into(),
            display_name: "Picoo Camera".into(),
            protocol_version: ALPN.into(),
            public_key: vec![4, 5, 6],
            pairing_required: true,
        };
        let mut buf = Vec::new();
        hello.encode(&mut buf).expect("encode hello");
        session
            .inject_control_for_test(bytes::Bytes::from(buf))
            .expect("inject hello");

        let challenge = PairingChallenge {
            short_code: "123456".into(),
            challenge_nonce: b"nonce".to_vec(),
        };
        let mut buf = Vec::new();
        challenge.encode(&mut buf).expect("encode challenge");
        session
            .inject_control_for_test(bytes::Bytes::from(buf))
            .expect("inject challenge");

        session
            .send_pairing_confirm("windows-receiver")
            .expect("confirm");

        let loaded = TrustedDeviceStore::load_from_path(&store_path).expect("load");
        assert!(loaded.is_paired("windows-receiver"));
    }
}
