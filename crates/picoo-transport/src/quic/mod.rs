//! quiche-backed QUIC session driver for tests and future production I/O.

use std::collections::HashMap;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

use picoo_quiche::{build_client_config, build_server_config, quiche};
use ring::rand::{SecureRandom, SystemRandom};
use thiserror::Error;

pub const CONTROL_STREAM_ID: u64 = 0;

const MAX_PACKET: usize = 1350;

#[derive(Debug, Error)]
pub enum QuicTransportError {
    #[error("quiche error: {0}")]
    Quiche(#[from] quiche::Error),
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("config error: {0}")]
    Config(#[from] picoo_quiche::QuicConfigError),
    #[error("handshake timed out")]
    HandshakeTimeout,
    #[error("not established")]
    NotEstablished,
}

struct ClientState {
    conn: quiche::Connection,
    peer_addr: SocketAddr,
}

pub struct QuicServer {
    socket: UdpSocket,
    config: quiche::Config,
    clients: HashMap<Vec<u8>, ClientState>,
}

pub struct QuicClient {
    socket: UdpSocket,
    server_addr: SocketAddr,
    conn: quiche::Connection,
}

pub struct QuicLoopback {
    pub server: QuicServer,
    pub client: QuicClient,
}

fn random_conn_id() -> quiche::ConnectionId<'static> {
    let mut buf = [0u8; quiche::MAX_CONN_ID_LEN];
    SystemRandom::new().fill(&mut buf).expect("random conn id");
    quiche::ConnectionId::from_vec(buf.to_vec())
}

fn flush_connection(
    socket: &UdpSocket,
    conn: &mut quiche::Connection,
    peer: SocketAddr,
) -> Result<(), QuicTransportError> {
    let mut out = [0u8; MAX_PACKET];
    loop {
        let (write, send_info) = match conn.send(&mut out) {
            Ok(v) => v,
            Err(quiche::Error::Done) => break,
            Err(e) => return Err(e.into()),
        };
        if write > 0 {
            let dest = if send_info.to == socket.local_addr().unwrap_or(peer) {
                peer
            } else {
                send_info.to
            };
            let _ = socket.send_to(&out[..write], dest);
        }
    }
    Ok(())
}

impl QuicServer {
    pub fn bind(addr: SocketAddr) -> Result<Self, QuicTransportError> {
        let socket = UdpSocket::bind(addr)?;
        socket.set_nonblocking(true)?;
        let config = build_server_config()?;
        Ok(Self {
            socket,
            config,
            clients: HashMap::new(),
        })
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    pub fn drive(&mut self) -> Result<(), QuicTransportError> {
        let local_addr = self.socket.local_addr()?;
        let mut buf = [0u8; 65535];

        for client in self.clients.values_mut() {
            if client.conn.timeout().is_some() {
                client.conn.on_timeout();
            }
        }

        loop {
            let (len, from) = match self.socket.recv_from(&mut buf) {
                Ok(v) => v,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e.into()),
            };

            let hdr = match quiche::Header::from_slice(&mut buf[..len], quiche::MAX_CONN_ID_LEN) {
                Ok(v) => v,
                Err(_) => continue,
            };

            let dcid = hdr.dcid.as_ref().to_vec();
            let info = quiche::RecvInfo {
                to: local_addr,
                from,
            };

            if let Some(client) = self.clients.get_mut(&dcid) {
                client.conn.recv(&mut buf[..len], info)?;
                continue;
            }

            if hdr.ty != quiche::Type::Initial {
                continue;
            }

            let scid = random_conn_id();
            let server_cid = scid.as_ref().to_vec();
            let mut conn = quiche::accept(&scid, None, local_addr, from, &mut self.config)?;
            conn.recv(&mut buf[..len], info)?;
            self.clients.insert(
                server_cid,
                ClientState {
                    conn,
                    peer_addr: from,
                },
            );
        }

        for client in self.clients.values_mut() {
            flush_connection(&self.socket, &mut client.conn, client.peer_addr)?;
        }

        Ok(())
    }

    fn established_client(&mut self) -> Result<&mut ClientState, QuicTransportError> {
        self.clients
            .values_mut()
            .find(|c| c.conn.is_established())
            .ok_or(QuicTransportError::NotEstablished)
    }

    pub fn is_established(&self) -> bool {
        self.clients.values().any(|c| c.conn.is_established())
    }

    pub fn send_stream(&mut self, stream_id: u64, data: &[u8]) -> Result<(), QuicTransportError> {
        let peer = {
            let client = self.established_client()?;
            client.conn.stream_send(stream_id, data, true)?;
            client.peer_addr
        };
        let socket = self.socket.try_clone()?;
        let client = self.established_client()?;
        flush_connection(&socket, &mut client.conn, peer)?;
        Ok(())
    }

    pub fn recv_stream(&mut self) -> Result<Option<(u64, Vec<u8>)>, QuicTransportError> {
        let client = match self.established_client() {
            Ok(c) => c,
            Err(_) => return Ok(None),
        };

        let stream_id = match client.conn.stream_readable_next() {
            Some(id) => id,
            None => return Ok(None),
        };

        let mut buf = [0u8; 4096];
        let (read, _fin) = client.conn.stream_recv(stream_id, &mut buf)?;
        Ok(Some((stream_id, buf[..read].to_vec())))
    }

    pub fn send_dgram(&mut self, data: &[u8]) -> Result<(), QuicTransportError> {
        let peer = {
            let client = self.established_client()?;
            client.conn.dgram_send(data)?;
            client.peer_addr
        };
        let socket = self.socket.try_clone()?;
        let client = self.established_client()?;
        flush_connection(&socket, &mut client.conn, peer)?;
        Ok(())
    }

    pub fn recv_dgram(&mut self) -> Result<Option<Vec<u8>>, QuicTransportError> {
        let client = match self.established_client() {
            Ok(c) => c,
            Err(_) => return Ok(None),
        };

        let mut buf = [0u8; MAX_PACKET];
        match client.conn.dgram_recv(&mut buf) {
            Ok(len) => Ok(Some(buf[..len].to_vec())),
            Err(quiche::Error::Done) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

impl QuicClient {
    pub fn connect(server_addr: SocketAddr) -> Result<Self, QuicTransportError> {
        let socket = UdpSocket::bind("127.0.0.1:0")?;
        socket.set_nonblocking(true)?;
        let local_addr = socket.local_addr()?;

        let mut config = build_client_config()?;
        let scid = random_conn_id();
        let conn = quiche::connect(
            Some("picoo-camera-test"),
            &scid,
            local_addr,
            server_addr,
            &mut config,
        )?;

        Ok(Self {
            socket,
            server_addr,
            conn,
        })
    }

    pub fn drive(&mut self) -> Result<(), QuicTransportError> {
        if self.conn.timeout().is_some() {
            self.conn.on_timeout();
        }

        flush_connection(&self.socket, &mut self.conn, self.server_addr)?;

        let mut buf = [0u8; 65535];
        loop {
            let (len, from) = match self.socket.recv_from(&mut buf) {
                Ok(v) => v,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e.into()),
            };
            let local = self.socket.local_addr()?;
            let info = quiche::RecvInfo { to: local, from };
            self.conn.recv(&mut buf[..len], info)?;
        }

        flush_connection(&self.socket, &mut self.conn, self.server_addr)?;
        Ok(())
    }

    pub fn is_established(&self) -> bool {
        self.conn.is_established()
    }

    pub fn send_stream(&mut self, stream_id: u64, data: &[u8]) -> Result<(), QuicTransportError> {
        self.conn.stream_send(stream_id, data, true)?;
        flush_connection(&self.socket, &mut self.conn, self.server_addr)?;
        Ok(())
    }

    pub fn recv_stream(&mut self) -> Result<Option<(u64, Vec<u8>)>, QuicTransportError> {
        let stream_id = match self.conn.stream_readable_next() {
            Some(id) => id,
            None => return Ok(None),
        };

        let mut buf = [0u8; 4096];
        let (read, _fin) = self.conn.stream_recv(stream_id, &mut buf)?;
        Ok(Some((stream_id, buf[..read].to_vec())))
    }

    pub fn send_dgram(&mut self, data: &[u8]) -> Result<(), QuicTransportError> {
        self.conn.dgram_send(data)?;
        flush_connection(&self.socket, &mut self.conn, self.server_addr)?;
        Ok(())
    }

    pub fn recv_dgram(&mut self) -> Result<Option<Vec<u8>>, QuicTransportError> {
        let mut buf = [0u8; MAX_PACKET];
        match self.conn.dgram_recv(&mut buf) {
            Ok(len) => Ok(Some(buf[..len].to_vec())),
            Err(quiche::Error::Done) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

pub fn establish_loopback() -> Result<QuicLoopback, QuicTransportError> {
    let server = QuicServer::bind("127.0.0.1:0".parse().unwrap())?;
    let server_addr = server.local_addr()?;
    let mut client = QuicClient::connect(server_addr)?;
    let mut server = server;

    for _ in 0..500 {
        client.drive()?;
        server.drive()?;
        if client.is_established() && server.is_established() {
            return Ok(QuicLoopback { server, client });
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    Err(QuicTransportError::HandshakeTimeout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use picoo_protocol::{VideoPacket, VideoPacketFlags};

    fn pump_until<F>(max: usize, mut f: F) -> bool
    where
        F: FnMut() -> bool,
    {
        for _ in 0..max {
            if f() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        false
    }

    fn drive(pair: &mut QuicLoopback) {
        pair.client.drive().unwrap();
        pair.server.drive().unwrap();
    }

    #[test]
    fn loopback_handshake_and_exchange_control_and_datagram() {
        let mut pair = establish_loopback().expect("handshake");

        pair.client
            .send_stream(CONTROL_STREAM_ID, b"client-hello")
            .expect("send control");

        let mut control = None;
        assert!(pump_until(100, || {
            drive(&mut pair);
            if let Ok(Some(v)) = pair.server.recv_stream() {
                control = Some(v);
                true
            } else {
                false
            }
        }));

        let (stream_id, data) = control.expect("control data");
        assert_eq!(stream_id, CONTROL_STREAM_ID);
        assert_eq!(&data[..12], b"client-hello");

        pair.server
            .send_stream(CONTROL_STREAM_ID, b"server-hello")
            .expect("server send");

        let mut reply = None;
        assert!(pump_until(100, || {
            drive(&mut pair);
            if let Ok(Some(v)) = pair.client.recv_stream() {
                reply = Some(v);
                true
            } else {
                false
            }
        }));

        let (_, data) = reply.expect("server hello");
        assert_eq!(&data[..12], b"server-hello");

        let packet = VideoPacket {
            version: VideoPacket::VERSION,
            flags: VideoPacketFlags::KEYFRAME,
            stream_epoch: 1,
            frame_id: 1,
            pts_us: 0,
            fragment_index: 0,
            fragment_count: 1,
            payload: Bytes::from_static(b"h264"),
        };
        let encoded = packet.encode().expect("encode");
        pair.client.send_dgram(&encoded).expect("dgram send");

        let mut video = None;
        assert!(pump_until(100, || {
            drive(&mut pair);
            if let Ok(Some(v)) = pair.server.recv_dgram() {
                video = Some(v);
                true
            } else {
                false
            }
        }));

        let raw = video.expect("video dgram");
        let decoded = VideoPacket::decode(&raw).expect("decode packet");
        assert_eq!(decoded.payload.as_ref(), b"h264");
    }
}
