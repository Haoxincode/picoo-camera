use picoo_protocol::ALPN;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use thiserror::Error;

const MAX_DATAGRAM: usize = 1350;

#[derive(Debug, Error)]
pub enum QuicConfigError {
    #[error("quiche config error: {0}")]
    Quiche(#[from] quiche::Error),
    #[error("certificate error: {0}")]
    Cert(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

struct TestCertPaths {
    cert: PathBuf,
    key: PathBuf,
}

static TEST_CERT_PATHS: OnceLock<TestCertPaths> = OnceLock::new();
static TEST_CERT_INIT: Mutex<()> = Mutex::new(());

fn test_cert_paths() -> Result<&'static TestCertPaths, QuicConfigError> {
    if let Some(paths) = TEST_CERT_PATHS.get() {
        return Ok(paths);
    }

    let _guard = TEST_CERT_INIT
        .lock()
        .map_err(|_| QuicConfigError::Cert("cert init lock poisoned".into()))?;
    if let Some(paths) = TEST_CERT_PATHS.get() {
        return Ok(paths);
    }

    let cert = rcgen::generate_simple_self_signed(vec!["picoo-camera-test".into()])
        .map_err(|e| QuicConfigError::Cert(e.to_string()))?;

    let dir = std::env::temp_dir().join("picoo-quiche-test-certs");
    std::fs::create_dir_all(&dir)?;
    let cert_path = dir.join("cert.pem");
    let key_path = dir.join("key.pem");
    std::fs::write(&cert_path, cert.cert.pem())?;
    std::fs::write(&key_path, cert.key_pair.serialize_pem())?;

    let _ = TEST_CERT_PATHS.set(TestCertPaths {
        cert: cert_path,
        key: key_path,
    });
    Ok(TEST_CERT_PATHS.get().expect("cert paths initialized"))
}

fn alpn_bytes() -> Vec<u8> {
    let mut alpn = Vec::with_capacity(1 + ALPN.len());
    alpn.push(ALPN.len() as u8);
    alpn.extend_from_slice(ALPN.as_bytes());
    alpn
}

fn base_config(is_server: bool) -> Result<quiche::Config, QuicConfigError> {
    let mut config = quiche::Config::new(quiche::PROTOCOL_VERSION)?;
    config.set_application_protos(&[alpn_bytes().as_slice()])?;
    config.set_max_idle_timeout(30_000);
    config.set_max_recv_udp_payload_size(MAX_DATAGRAM);
    config.set_max_send_udp_payload_size(MAX_DATAGRAM);
    config.set_initial_max_data(10_000_000);
    config.set_initial_max_stream_data_bidi_local(1_000_000);
    config.set_initial_max_stream_data_bidi_remote(1_000_000);
    config.set_initial_max_stream_data_uni(1_000_000);
    config.set_initial_max_streams_bidi(100);
    config.set_initial_max_streams_uni(100);
    config.set_disable_active_migration(true);
    config.enable_dgram(true, 100, 100);

    if is_server {
        let paths = test_cert_paths()?;
        config.load_cert_chain_from_pem_file(
            paths
                .cert
                .to_str()
                .ok_or_else(|| QuicConfigError::Cert("invalid cert path".into()))?,
        )?;
        config.load_priv_key_from_pem_file(
            paths
                .key
                .to_str()
                .ok_or_else(|| QuicConfigError::Cert("invalid key path".into()))?,
        )?;
    } else {
        config.verify_peer(false);
    }

    Ok(config)
}

pub fn build_server_config() -> Result<quiche::Config, QuicConfigError> {
    base_config(true)
}

pub fn build_client_config() -> Result<quiche::Config, QuicConfigError> {
    base_config(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_and_client_configs_build() {
        build_server_config().expect("server config");
        build_client_config().expect("client config");
    }
}

#[cfg(test)]
mod loopback {
    use super::{build_client_config, build_server_config};
    use crate::quiche;
    use ring::rand::{SecureRandom, SystemRandom};
    use std::net::UdpSocket;
    use std::time::Duration;

    fn conn_id() -> quiche::ConnectionId<'static> {
        let mut buf = [0u8; quiche::MAX_CONN_ID_LEN];
        SystemRandom::new().fill(&mut buf).unwrap();
        quiche::ConnectionId::from_vec(buf.to_vec())
    }

    #[test]
    fn raw_quiche_loopback_handshake() {
        let server_sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let server_addr = server_sock.local_addr().unwrap();
        server_sock.set_nonblocking(true).unwrap();

        let client_sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        client_sock.set_nonblocking(true).unwrap();
        let client_addr = client_sock.local_addr().unwrap();

        let mut server_config = build_server_config().expect("server cfg");
        let mut client_config = build_client_config().expect("client cfg");

        let scid = conn_id();
        let mut client_conn = quiche::connect(
            Some("picoo-camera-test"),
            &scid,
            client_addr,
            server_addr,
            &mut client_config,
        )
        .expect("connect");

        let mut server_conn: Option<quiche::Connection> = None;
        let mut buf = [0u8; 65535];
        let mut out = [0u8; 1350];

        for _ in 0..500 {
            if client_conn.is_closed() {
                panic!("client closed: {:?}", client_conn.stats());
            }

            if client_conn.timeout().is_some() {
                client_conn.on_timeout();
                if let Some(s) = server_conn.as_mut() {
                    s.on_timeout();
                }
            }

            loop {
                let (write, info) = match client_conn.send(&mut out) {
                    Ok(v) => v,
                    Err(quiche::Error::Done) => break,
                    Err(e) => panic!("client send: {e:?}"),
                };
                if write > 0 {
                    let _ = client_sock.send_to(&out[..write], info.to);
                }
            }

            if let Some(server) = server_conn.as_mut() {
                loop {
                    let (write, info) = match server.send(&mut out) {
                        Ok(v) => v,
                        Err(quiche::Error::Done) => break,
                        Err(e) => panic!("server send: {e:?}"),
                    };
                    if write > 0 {
                        let _ = server_sock.send_to(&out[..write], info.to);
                    }
                }
            }

            loop {
                let (len, from) = match client_sock.recv_from(&mut buf) {
                    Ok(v) => v,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) => panic!("client recv: {e:?}"),
                };
                let info = quiche::RecvInfo {
                    to: client_addr,
                    from,
                };
                if let Err(e) = client_conn.recv(&mut buf[..len], info) {
                    panic!("client recv process: {e:?}");
                }
            }

            loop {
                let (len, from) = match server_sock.recv_from(&mut buf) {
                    Ok(v) => v,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) => panic!("server recv: {e:?}"),
                };

                if server_conn.is_none() {
                    let _hdr = quiche::Header::from_slice(&mut buf[..len], quiche::MAX_CONN_ID_LEN)
                        .expect("header");
                    let scid = conn_id();
                    server_conn = Some(
                        quiche::accept(&scid, None, server_addr, from, &mut server_config)
                            .expect("accept"),
                    );
                }

                let info = quiche::RecvInfo {
                    to: server_addr,
                    from,
                };
                if let Err(e) = server_conn.as_mut().unwrap().recv(&mut buf[..len], info) {
                    panic!("server recv process: {e:?}");
                }
            }

            if client_conn.is_established()
                && server_conn.as_ref().is_some_and(|s| s.is_established())
            {
                return;
            }

            std::thread::sleep(Duration::from_millis(2));
        }

        panic!("handshake timeout");
    }
}
