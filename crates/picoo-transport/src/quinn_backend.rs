//! Quinn endpoint actors shared by sender and receiver transports.

use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{mpsc as std_mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use bytes::Bytes;
use picoo_protocol::VideoPacket;
use quinn::crypto::rustls::{QuicClientConfig, QuicServerConfig};
use quinn::{ClientConfig, Connection, Endpoint, ServerConfig, TransportConfig, VarInt};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use thiserror::Error;
use tokio::runtime::{Builder, Runtime};
use tokio::sync::mpsc;

use crate::control_framing::{encode_control_frame, ControlFrameDecoder};
use crate::{CloseReason, SessionId, TransportEvent, TransportLinkStats};

const COMMAND_CAPACITY: usize = 64;
// Capacity is measured in complete access units, not fragments. A deep video
// queue directly becomes glass-to-glass latency under congestion.
const VIDEO_COMMAND_CAPACITY: usize = 3;
const VIDEO_EVENT_CAPACITY: usize = 512;
const CONTROL_READ_BUFFER: usize = 4096;
const DATAGRAM_RECEIVE_BUFFER_SIZE: usize = 2 * 1024 * 1024;
// Bound queued one-way media to a small number of 1080p access units. A 2 MiB
// Datagram buffer can silently turn a LAN camera into seconds of stale video.
const DATAGRAM_SEND_BUFFER_SIZE: usize = 256 * 1024;

#[derive(Debug, Error)]
pub enum QuicTransportError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("QUIC configuration error: {0}")]
    Config(String),
    #[error("QUIC worker is unavailable")]
    WorkerUnavailable,
    #[error("QUIC command queue is full")]
    CommandBackpressure,
    #[error("QUIC video queue is full")]
    VideoBackpressure,
}

#[derive(Debug)]
pub(crate) enum Command {
    Connect {
        session: SessionId,
        server_addr: SocketAddr,
    },
    SendControl {
        session: SessionId,
        message: Bytes,
    },
    Close {
        session: SessionId,
        reason: CloseReason,
    },
}

#[derive(Debug)]
struct VideoCommand {
    session: SessionId,
    packets: Vec<VideoPacket>,
    enqueued_at: Instant,
}

#[derive(Default)]
struct SharedState {
    active_session: Option<SessionId>,
    stats: Option<TransportLinkStats>,
    video_queue_age_ms: f64,
    video_dropped_access_units: u64,
}

struct EventSender {
    critical: std_mpsc::Sender<TransportEvent>,
    video: std_mpsc::SyncSender<TransportEvent>,
}

impl Clone for EventSender {
    fn clone(&self) -> Self {
        Self {
            critical: self.critical.clone(),
            video: self.video.clone(),
        }
    }
}

impl EventSender {
    fn critical(&self, event: TransportEvent) {
        let _ = self.critical.send(event);
    }

    fn video(&self, event: TransportEvent) {
        // Video is intentionally lossy under consumer backpressure. Control and
        // lifecycle events use a separate reliable queue.
        let _ = self.video.try_send(event);
    }
}

pub(crate) struct TransportActor {
    endpoint: Endpoint,
    commands: mpsc::Sender<Command>,
    video_commands: mpsc::Sender<VideoCommand>,
    critical_events: std_mpsc::Receiver<TransportEvent>,
    video_events: std_mpsc::Receiver<TransportEvent>,
    state: Arc<Mutex<SharedState>>,
}

impl TransportActor {
    pub(crate) fn client(server_addr: SocketAddr) -> Result<Self, QuicTransportError> {
        let runtime = shared_runtime()?;
        let bind_addr = if server_addr.is_ipv4() {
            SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0)
        } else {
            SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0)
        };
        let mut endpoint = {
            let _runtime_guard = runtime.enter();
            Endpoint::client(bind_addr)?
        };
        endpoint.set_default_client_config(client_config()?);

        let (actor, command_rx, video_rx, events, state) = Self::channels(endpoint.clone());
        runtime.spawn(client_worker(endpoint, command_rx, video_rx, events, state));
        Ok(actor)
    }

    pub(crate) fn server(addr: SocketAddr) -> Result<Self, QuicTransportError> {
        let runtime = shared_runtime()?;
        let endpoint = {
            let _runtime_guard = runtime.enter();
            Endpoint::server(server_config()?, addr)?
        };
        let (actor, command_rx, video_rx, events, state) = Self::channels(endpoint.clone());
        runtime.spawn(server_worker(endpoint, command_rx, video_rx, events, state));
        Ok(actor)
    }

    fn channels(
        endpoint: Endpoint,
    ) -> (
        Self,
        mpsc::Receiver<Command>,
        mpsc::Receiver<VideoCommand>,
        EventSender,
        Arc<Mutex<SharedState>>,
    ) {
        let (command_tx, command_rx) = mpsc::channel(COMMAND_CAPACITY);
        let (video_tx, video_rx) = mpsc::channel(VIDEO_COMMAND_CAPACITY);
        let (critical_tx, critical_rx) = std_mpsc::channel();
        let (video_event_tx, video_event_rx) = std_mpsc::sync_channel(VIDEO_EVENT_CAPACITY);
        let state = Arc::new(Mutex::new(SharedState::default()));
        (
            Self {
                endpoint,
                commands: command_tx,
                video_commands: video_tx,
                critical_events: critical_rx,
                video_events: video_event_rx,
                state: Arc::clone(&state),
            },
            command_rx,
            video_rx,
            EventSender {
                critical: critical_tx,
                video: video_event_tx,
            },
            state,
        )
    }

    pub(crate) fn local_addr(&self) -> io::Result<SocketAddr> {
        self.endpoint.local_addr()
    }

    pub(crate) fn command(&self, command: Command) -> Result<(), QuicTransportError> {
        self.commands
            .try_send(command)
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => QuicTransportError::CommandBackpressure,
                mpsc::error::TrySendError::Closed(_) => QuicTransportError::WorkerUnavailable,
            })
    }

    pub(crate) fn send_video_batch(
        &self,
        session: SessionId,
        packets: Vec<VideoPacket>,
    ) -> Result<(), QuicTransportError> {
        self.video_commands
            .try_send(VideoCommand {
                session,
                packets,
                enqueued_at: Instant::now(),
            })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => {
                    let mut state = self.state.lock().expect("QUIC state mutex poisoned");
                    state.video_dropped_access_units =
                        state.video_dropped_access_units.saturating_add(1);
                    let dropped = state.video_dropped_access_units;
                    if let Some(stats) = state.stats.as_mut() {
                        stats.video_dropped_access_units = dropped;
                    }
                    QuicTransportError::VideoBackpressure
                }
                mpsc::error::TrySendError::Closed(_) => QuicTransportError::WorkerUnavailable,
            })
    }

    pub(crate) fn poll_event(&self) -> Option<TransportEvent> {
        self.critical_events
            .try_recv()
            .ok()
            .or_else(|| self.video_events.try_recv().ok())
    }

    pub(crate) fn active_session(&self) -> Option<SessionId> {
        self.state
            .lock()
            .expect("QUIC state mutex poisoned")
            .active_session
    }

    pub(crate) fn link_stats(&self) -> Option<TransportLinkStats> {
        self.state.lock().expect("QUIC state mutex poisoned").stats
    }
}

impl Drop for TransportActor {
    fn drop(&mut self) {
        self.endpoint
            .close(VarInt::from_u32(0), b"picoo-transport-drop");
    }
}

static QUIC_RUNTIME: OnceLock<Runtime> = OnceLock::new();

fn shared_runtime() -> Result<&'static Runtime, QuicTransportError> {
    if let Some(runtime) = QUIC_RUNTIME.get() {
        return Ok(runtime);
    }
    let runtime = Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("picoo-quic")
        .enable_all()
        .build()
        .map_err(QuicTransportError::Io)?;
    let _ = QUIC_RUNTIME.set(runtime);
    Ok(QUIC_RUNTIME
        .get()
        .expect("QUIC runtime initialized by this process"))
}

fn transport_config() -> Arc<TransportConfig> {
    let mut config = TransportConfig::default();
    config
        .max_concurrent_uni_streams(VarInt::from_u32(4))
        .keep_alive_interval(Some(Duration::from_secs(5)))
        .datagram_receive_buffer_size(Some(DATAGRAM_RECEIVE_BUFFER_SIZE))
        .datagram_send_buffer_size(DATAGRAM_SEND_BUFFER_SIZE);
    Arc::new(config)
}

fn server_config() -> Result<ServerConfig, QuicTransportError> {
    let certified = rcgen::generate_simple_self_signed(vec!["picoo-camera".into()])
        .map_err(|error| QuicTransportError::Config(error.to_string()))?;
    let certificate = CertificateDer::from(certified.cert);
    let key = PrivatePkcs8KeyDer::from(certified.key_pair.serialize_der());
    let mut tls = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![certificate], key.into())
        .map_err(|error| QuicTransportError::Config(error.to_string()))?;
    tls.alpn_protocols = vec![picoo_protocol::ALPN.as_bytes().to_vec()];

    let crypto = QuicServerConfig::try_from(tls)
        .map_err(|error| QuicTransportError::Config(error.to_string()))?;
    let mut config = ServerConfig::with_crypto(Arc::new(crypto));
    config.transport_config(transport_config());
    Ok(config)
}

fn client_config() -> Result<ClientConfig, QuicTransportError> {
    // picoo-pairing owns device authentication and key continuity. QUIC's
    // ephemeral certificate is transport encryption, not a second identity.
    let mut tls = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(SkipServerVerification::new())
        .with_no_client_auth();
    tls.alpn_protocols = vec![picoo_protocol::ALPN.as_bytes().to_vec()];

    let crypto = QuicClientConfig::try_from(tls)
        .map_err(|error| QuicTransportError::Config(error.to_string()))?;
    let mut config = ClientConfig::new(Arc::new(crypto));
    config.transport_config(transport_config());
    Ok(config)
}

#[derive(Debug)]
struct SkipServerVerification(Arc<rustls::crypto::CryptoProvider>);

impl SkipServerVerification {
    fn new() -> Arc<Self> {
        Arc::new(Self(Arc::new(rustls::crypto::ring::default_provider())))
    }
}

impl rustls::client::danger::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            signature,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            signature,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

async fn client_worker(
    endpoint: Endpoint,
    mut commands: mpsc::Receiver<Command>,
    mut video: mpsc::Receiver<VideoCommand>,
    events: EventSender,
    state: Arc<Mutex<SharedState>>,
) {
    while let Some(command) = commands.recv().await {
        let Command::Connect {
            session,
            server_addr,
        } = command
        else {
            continue;
        };

        let connecting = match endpoint.connect(server_addr, "picoo-camera") {
            Ok(connecting) => connecting,
            Err(error) => {
                events.critical(TransportEvent::Disconnected(
                    session,
                    CloseReason::Error(error.to_string()),
                ));
                continue;
            }
        };
        match connecting.await {
            Ok(connection) => {
                run_connection(
                    connection,
                    session,
                    &mut commands,
                    &mut video,
                    &events,
                    &state,
                )
                .await;
            }
            Err(error) => events.critical(TransportEvent::Disconnected(
                session,
                CloseReason::Error(error.to_string()),
            )),
        }
    }
}

async fn server_worker(
    endpoint: Endpoint,
    mut commands: mpsc::Receiver<Command>,
    mut video: mpsc::Receiver<VideoCommand>,
    events: EventSender,
    state: Arc<Mutex<SharedState>>,
) {
    let mut next_session = 1_u64;
    loop {
        tokio::select! {
            incoming = endpoint.accept() => {
                let Some(incoming) = incoming else { return; };
                match incoming.await {
                    Ok(connection) => {
                        let session = SessionId(next_session);
                        next_session += 1;
                        run_connection(
                            connection,
                            session,
                            &mut commands,
                            &mut video,
                            &events,
                            &state,
                        ).await;
                    }
                    Err(error) => {
                        let session = SessionId(next_session);
                        next_session += 1;
                        events.critical(TransportEvent::Disconnected(
                            session,
                            CloseReason::Error(error.to_string()),
                        ));
                    }
                }
            }
            command = commands.recv() => {
                if command.is_none() { return; }
            }
        }
    }
}

enum Inbound {
    Control(Vec<u8>),
    Datagrams(Vec<Bytes>),
}

async fn receive_control(connection: Connection, inbound: mpsc::Sender<Inbound>) {
    let mut stream = match connection.accept_uni().await {
        Ok(stream) => stream,
        Err(_) => return,
    };
    let mut buffer = vec![0_u8; CONTROL_READ_BUFFER];
    while let Ok(Some(read)) = stream.read(&mut buffer).await {
        if inbound
            .send(Inbound::Control(buffer[..read].to_vec()))
            .await
            .is_err()
        {
            return;
        }
    }
}

async fn receive_datagrams(connection: Connection, inbound: mpsc::Sender<Inbound>) {
    const MAX_BATCH_DATAGRAMS: usize = 64;
    const MAX_BATCH_WAIT: Duration = Duration::from_millis(1);

    while let Ok(first) = connection.read_datagram().await {
        let mut batch = Vec::with_capacity(MAX_BATCH_DATAGRAMS);
        batch.push(first);
        let deadline = tokio::time::Instant::now() + MAX_BATCH_WAIT;
        while batch.len() < MAX_BATCH_DATAGRAMS {
            match tokio::time::timeout_at(deadline, connection.read_datagram()).await {
                Ok(Ok(datagram)) => batch.push(datagram),
                Ok(Err(_)) | Err(_) => break,
            }
        }
        if inbound.send(Inbound::Datagrams(batch)).await.is_err() {
            return;
        }
    }
}

async fn run_connection(
    connection: Connection,
    session: SessionId,
    commands: &mut mpsc::Receiver<Command>,
    video: &mut mpsc::Receiver<VideoCommand>,
    events: &EventSender,
    state: &Arc<Mutex<SharedState>>,
) {
    {
        let mut shared = state.lock().expect("QUIC state mutex poisoned");
        shared.active_session = Some(session);
        let stats = link_stats(&connection, &shared);
        shared.stats = Some(stats);
    }
    events.critical(TransportEvent::Connected(session));

    let (inbound_tx, mut inbound_rx) = mpsc::channel(COMMAND_CAPACITY);
    let control_task = tokio::spawn(receive_control(connection.clone(), inbound_tx.clone()));
    let datagram_task = tokio::spawn(receive_datagrams(connection.clone(), inbound_tx));
    let mut control_tx: Option<quinn::SendStream> = None;
    let mut control_decoder = ControlFrameDecoder::default();
    let mut stats_tick = tokio::time::interval(Duration::from_millis(250));

    let disconnect_reason = loop {
        tokio::select! {
            biased;
            command = commands.recv() => {
                match command {
                    Some(Command::SendControl { session: target, message }) if target == session => {
                        let frame = match encode_control_frame(&message) {
                            Ok(frame) => frame,
                            Err(error) => break CloseReason::Error(error.to_string()),
                        };
                        if control_tx.is_none() {
                            match connection.open_uni().await {
                                Ok(stream) => control_tx = Some(stream),
                                Err(error) => break CloseReason::Error(error.to_string()),
                            }
                        }
                        if let Err(error) = control_tx
                            .as_mut()
                            .expect("control stream initialized")
                            .write_all(&frame)
                            .await
                        {
                            break CloseReason::Error(error.to_string());
                        }
                    }
                    Some(Command::Close { session: target, reason }) if target == session => {
                        if let Some(stream) = control_tx.as_mut() {
                            let _ = stream.finish();
                            let _ = tokio::time::timeout(
                                Duration::from_millis(250),
                                stream.stopped(),
                            )
                            .await;
                        }
                        connection.close(VarInt::from_u32(0), b"picoo-close");
                        break reason;
                    }
                    Some(Command::Connect { session: other, .. }) => {
                        events.critical(TransportEvent::Disconnected(
                            other,
                            CloseReason::Error("another QUIC session is already active".into()),
                        ));
                    }
                    Some(_) => {}
                    None => break CloseReason::LocalClose,
                }
            }
            command = video.recv() => {
                match command {
                    Some(VideoCommand { session: target, packets, enqueued_at }) if target == session => {
                        {
                            let mut shared = state.lock().expect("QUIC state mutex poisoned");
                            shared.video_queue_age_ms = enqueued_at.elapsed().as_secs_f64() * 1_000.0;
                        }
                        let encoded = match packets
                            .into_iter()
                            .map(|packet| packet.encode())
                            .collect::<Result<Vec<_>, _>>()
                        {
                            Ok(encoded) => encoded,
                            Err(error) => break CloseReason::Error(error.to_string()),
                        };
                        let required = encoded.iter().map(Bytes::len).sum::<usize>();
                        // Quinn evicts oldest individual datagrams when its send buffer
                        // fills. Every AU, including a keyframe, must therefore fit in the
                        // currently available space before its first fragment is enqueued.
                        // Letting a keyframe evict arbitrary old datagrams corrupts those
                        // older AUs and can trigger another reference-chain recovery after
                        // the keyframe has already repaired the decoder.
                        let available = connection.datagram_send_buffer_space();
                        if should_enqueue_access_unit(available, required) {
                            if let Err(error) = encoded
                                .into_iter()
                                .try_for_each(|datagram| connection.send_datagram(datagram))
                            {
                                break CloseReason::Error(error.to_string());
                            }
                        } else {
                            let mut shared = state.lock().expect("QUIC state mutex poisoned");
                            shared.video_dropped_access_units =
                                shared.video_dropped_access_units.saturating_add(1);
                        }
                    }
                    Some(_) => {}
                    None => break CloseReason::LocalClose,
                }
            }
            inbound = inbound_rx.recv() => {
                match inbound {
                    Some(Inbound::Control(chunk)) => {
                        control_decoder.push(&chunk);
                        match control_decoder.drain_messages() {
                            Ok(messages) => {
                                for message in messages {
                                    events.critical(TransportEvent::ControlMessage(session, message));
                                }
                            }
                            Err(error) => break CloseReason::Error(error.to_string()),
                        }
                    }
                    Some(Inbound::Datagrams(datagrams)) => {
                        let packets = datagrams
                            .into_iter()
                            .filter_map(|datagram| VideoPacket::decode(&datagram).ok())
                            .collect::<Vec<_>>();
                        if !packets.is_empty() {
                            events.video(TransportEvent::VideoPackets(session, packets));
                        }
                    }
                    None => break CloseReason::PeerClose,
                }
            }
            _ = stats_tick.tick() => {
                let mut shared = state.lock().expect("QUIC state mutex poisoned");
                let stats = link_stats(&connection, &shared);
                shared.stats = Some(stats);
            }
            _ = connection.closed() => break CloseReason::PeerClose,
        }
    };

    control_task.abort();
    datagram_task.abort();
    connection.close(VarInt::from_u32(0), b"picoo-session-end");
    {
        let mut shared = state.lock().expect("QUIC state mutex poisoned");
        if shared.active_session == Some(session) {
            shared.active_session = None;
            shared.stats = None;
        }
    }
    events.critical(TransportEvent::Disconnected(session, disconnect_reason));
}

fn should_enqueue_access_unit(available: usize, required: usize) -> bool {
    available >= required
}

fn link_stats(connection: &Connection, shared: &SharedState) -> TransportLinkStats {
    let stats = connection.stats();
    TransportLinkStats {
        rtt_ms: stats.path.rtt.as_secs_f64() * 1_000.0,
        lost_packets: stats.path.lost_packets,
        sent_packets: stats.path.sent_packets,
        recv_packets: stats.udp_rx.datagrams,
        dgram_recv: stats.frame_rx.datagram,
        video_queue_age_ms: shared.video_queue_age_ms,
        video_dropped_access_units: shared.video_dropped_access_units,
        video_buffered_bytes: DATAGRAM_SEND_BUFFER_SIZE
            .saturating_sub(connection.datagram_send_buffer_space())
            as u64,
    }
}

#[cfg(test)]
mod access_unit_queue_tests {
    use super::*;

    #[test]
    fn congested_delta_is_dropped_as_a_complete_access_unit() {
        assert!(!should_enqueue_access_unit(1_000, 1_001));
        assert!(should_enqueue_access_unit(1_001, 1_001));
    }

    #[test]
    fn recovery_keyframe_cannot_evict_fragments_from_older_access_units() {
        assert!(!should_enqueue_access_unit(0, DATAGRAM_SEND_BUFFER_SIZE));
        assert!(!should_enqueue_access_unit(
            DATAGRAM_SEND_BUFFER_SIZE - 1,
            DATAGRAM_SEND_BUFFER_SIZE
        ));
        assert!(should_enqueue_access_unit(
            DATAGRAM_SEND_BUFFER_SIZE,
            DATAGRAM_SEND_BUFFER_SIZE
        ));
    }
}
