use super::*;

#[test]
fn rejects_zero_platform_network_identifiers() {
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").expect("socket");
    assert_eq!(
        apply_client_network_binding(
            &socket,
            ClientNetworkBinding::AndroidNetwork {
                network_handle: 0,
                allow_system_lan_route_fallback: false,
            },
        )
        .expect_err("zero Android handle")
        .kind(),
        io::ErrorKind::InvalidInput
    );
    assert_eq!(
        apply_client_network_binding(&socket, ClientNetworkBinding::AppleInterface(0))
            .expect_err("zero Apple interface")
            .kind(),
        io::ErrorKind::InvalidInput
    );
}

#[cfg(target_os = "macos")]
#[test]
fn binds_an_apple_udp_socket_to_loopback_interface() {
    let name = std::ffi::CString::new("lo0").expect("interface name");
    let index = unsafe { libc::if_nametoindex(name.as_ptr()) };
    assert_ne!(index, 0, "lo0 interface index");
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").expect("socket");
    apply_client_network_binding(&socket, ClientNetworkBinding::AppleInterface(index))
        .expect("IP_BOUND_IF");
}

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

#[tokio::test]
async fn unrelated_alpn_cannot_establish_a_picoo_connection() {
    // REQ-PICOO-PROTOCOL-014: exercise real TLS negotiation, not string comparison.
    let server =
        Endpoint::server(server_config().unwrap(), "127.0.0.1:0".parse().unwrap()).unwrap();
    let mut client = Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
    let mut tls = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(SkipServerVerification::new())
        .with_no_client_auth();
    tls.alpn_protocols = vec![b"unrelated-protocol".to_vec()];
    client.set_default_client_config(ClientConfig::new(Arc::new(
        QuicClientConfig::try_from(tls).unwrap(),
    )));
    let connecting = client
        .connect(server.local_addr().unwrap(), "picoo-camera")
        .unwrap();
    let attempt = async {
        let (client_result, server_result) =
            tokio::join!(connecting, async { server.accept().await.unwrap().await });
        assert!(client_result.is_err(), "unrelated ALPN connected");
        assert!(server_result.is_err(), "server accepted unrelated ALPN");
    };
    tokio::time::timeout(Duration::from_secs(5), attempt)
        .await
        .expect("handshake deadline");
    client.close(VarInt::from_u32(0), b"test-complete");
    server.close(VarInt::from_u32(0), b"test-complete");
}
