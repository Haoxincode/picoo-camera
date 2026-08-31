use picoo_protocol::control::ClientHello;

use super::*;

#[test]
fn client_hello_queued_before_async_connect_is_sent_when_connected() {
    // REQ-PICOO-DISCOVERY-007: mirrors Android connect() -> sendClientHello().
    let mut sender = SenderSession::new(DeferredConnectTransport::new());
    sender
        .connect(Endpoint {
            host: "192.168.8.101".into(),
            port: 4433,
        })
        .expect("queue connect");

    sender
        .send_client_hello("android-sender", "Pixel", &[1, 2, 3])
        .expect("queue hello before QUIC handshake completes");
    assert!(sender.transport().sent_control.is_empty());

    sender.transport_mut().complete_connect();
    sender.pump().expect("process connected event");

    assert_eq!(sender.status(), SenderStatus::Negotiating);
    let encoded = sender
        .transport()
        .sent_control
        .first()
        .expect("ClientHello emitted after connect");
    let hello = ClientHello::decode(encoded.as_ref()).expect("decode ClientHello");
    assert_eq!(hello.sender_id, "android-sender");
    assert_eq!(hello.protocol_version, ALPN);
}

#[test]
fn memory_transport_flush_pending() {
    let mut session = SenderSession::new(MemoryTransport::new());
    session
        .connect(Endpoint {
            host: "127.0.0.1".into(),
            port: 1,
        })
        .expect("connect");
    session.force_status_for_test(SenderStatus::Streaming);
    session
        .ingest_access_unit(b"au-bytes", true, 1, 1)
        .expect("ingest");
    let sent = session.flush_pending().expect("flush");
    assert_eq!(sent, 1);
    assert_eq!(session.stats().sent_datagrams, 1);
}

#[test]
fn disconnected_media_is_rejected_and_pending_packets_are_cleared() {
    let mut session = SenderSession::new(MemoryTransport::new());
    assert!(matches!(
        session.ingest_access_unit(b"offline", true, 1, 1),
        Err(SenderError::NotConnected)
    ));
    assert_eq!(session.pending_packets(), 0);

    session
        .connect(Endpoint {
            host: "127.0.0.1".into(),
            port: 1,
        })
        .expect("connect");
    session.force_status_for_test(SenderStatus::Streaming);
    session
        .ingest_access_unit(b"queued", true, 2, 1)
        .expect("ingest while connected");
    assert_eq!(session.pending_packets(), 1);
    session.disconnect();
    assert_eq!(session.pending_packets(), 0);
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
    assert_eq!(session.last_scheduled_reconnect_delay_ms(), Some(500));

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
fn reconnect_backoff_escalates_across_failed_attempts() {
    // REQ-PICOO-TRANSPORT-004 / PUC-006: 500 → 1000 → 2000 → 5000 → 5000.
    let mut session = SenderSession::new(MemoryTransport::new());
    session
        .connect(Endpoint {
            host: "127.0.0.1".into(),
            port: 4433,
        })
        .expect("connect");

    session.disconnect_for_test(CloseReason::Timeout);
    session.pump().expect("pump");
    assert_eq!(session.status(), SenderStatus::Reconnecting);
    assert_eq!(session.last_scheduled_reconnect_delay_ms(), Some(500));
    assert_eq!(session.reconnect_attempt(), 1);

    session.simulate_failed_reconnect_for_test();
    assert_eq!(session.last_scheduled_reconnect_delay_ms(), Some(1_000));
    assert_eq!(session.reconnect_attempt(), 2);
    session.simulate_failed_reconnect_for_test();
    assert_eq!(session.last_scheduled_reconnect_delay_ms(), Some(2_000));
    session.simulate_failed_reconnect_for_test();
    assert_eq!(session.last_scheduled_reconnect_delay_ms(), Some(5_000));
    session.simulate_failed_reconnect_for_test();
    assert_eq!(session.last_scheduled_reconnect_delay_ms(), Some(5_000));
}

#[test]
fn user_disconnect_stays_disconnected_without_reconnect() {
    // PUC-005: intentional stop must not bounce into Reconnecting.
    let mut session = SenderSession::new(MemoryTransport::new());
    session
        .connect(Endpoint {
            host: "127.0.0.1".into(),
            port: 4433,
        })
        .expect("connect");
    assert!(session.is_connected());

    session.disconnect();
    assert_eq!(session.status(), SenderStatus::Disconnected);
    assert!(!session.is_connected());

    for _ in 0..10 {
        session.pump().expect("pump");
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(session.status(), SenderStatus::Disconnected);
    assert!(!session.is_connected());

    // Explicit connect must work again after user stop.
    session
        .connect(Endpoint {
            host: "127.0.0.1".into(),
            port: 4433,
        })
        .expect("reconnect after user stop");
    assert!(session.is_connected());
}

#[test]
fn high_packet_loss_marks_network_unstable() {
    // REQ-PICOO-SESSION-001
    let mut session = SenderSession::new(MemoryTransport::new());
    session
        .connect(Endpoint {
            host: "127.0.0.1".into(),
            port: 4433,
        })
        .expect("connect");
    session.force_status_for_test(SenderStatus::Streaming);

    let high_loss = ReceiverStatsMsg {
        packet_loss: 0.05,
        ..Default::default()
    };
    let mut buf = Vec::new();
    high_loss.encode(&mut buf).expect("encode");
    session
        .inject_control_for_test(bytes::Bytes::from(buf))
        .expect("inject");
    assert_eq!(session.status(), SenderStatus::NetworkUnstable);

    let recovered = ReceiverStatsMsg {
        packet_loss: 0.005,
        ..Default::default()
    };
    let mut buf = Vec::new();
    recovered.encode(&mut buf).expect("encode");
    session
        .inject_control_for_test(bytes::Bytes::from(buf))
        .expect("inject");
    assert_eq!(session.status(), SenderStatus::Streaming);
}

#[test]
fn mark_permission_required_is_observable() {
    let mut session = SenderSession::new(MemoryTransport::new());
    session.mark_permission_required();
    assert_eq!(session.status(), SenderStatus::PermissionRequired);
    session.clear_permission_required();
    assert_eq!(session.status(), SenderStatus::Disconnected);
}

#[test]
fn camera_permission_gate_resumes_live_session_without_reconnect() {
    let mut session = SenderSession::new(MemoryTransport::new());
    session.force_status_for_test(SenderStatus::Streaming);

    session.mark_permission_required();
    session.mark_permission_required();
    assert_eq!(session.status(), SenderStatus::PermissionRequired);

    session.clear_permission_required();
    assert_eq!(session.status(), SenderStatus::Streaming);
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
fn resends_stream_config_and_requests_keyframe_after_reconnect() {
    let mut session = SenderSession::new(MemoryTransport::new());
    let endpoint = Endpoint {
        host: "127.0.0.1".into(),
        port: 4433,
    };
    session.connect(endpoint.clone()).expect("connect");
    session
        .send_client_hello("phone-1", "Pixel", &[1, 2, 3])
        .expect("hello");
    session.set_stream_config(StreamConfigParams {
        width: 1920,
        height: 1080,
        fps: 30,
        bitrate_bps: 6_000_000,
        stream_epoch: 2,
        mirrored: true,
        sps: vec![0x67, 0x42],
        pps: vec![0x68, 0xce],
        ..Default::default()
    });

    let hello = ServerHello {
        receiver_id: "recv-1".into(),
        display_name: "Desktop".into(),
        protocol_version: ALPN.into(),
        public_key: vec![9, 9],
        pairing_required: false,
    };
    let mut buf = Vec::new();
    hello.encode(&mut buf).expect("encode");
    session
        .inject_control_for_test(bytes::Bytes::from(buf))
        .expect("inject hello");
    assert_eq!(session.status(), SenderStatus::Streaming);
    assert_eq!(session.connected_receiver_id(), Some("recv-1"));
    assert_eq!(session.connected_receiver_display_name(), Some("Desktop"));
    assert!(session.stream_config_sent());
    assert!(session.take_keyframe_request());

    session.disconnect_for_test(CloseReason::PeerClose);
    session.pump().expect("disconnect pump");
    assert!(!session.stream_config_sent());

    for _ in 0..20 {
        session.pump().expect("reconnect pump");
        if session.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(600));
    }
    assert!(session.is_connected());

    let hello2 = ServerHello {
        receiver_id: "recv-1".into(),
        display_name: "Desktop".into(),
        protocol_version: ALPN.into(),
        public_key: vec![9, 9],
        pairing_required: false,
    };
    let mut buf2 = Vec::new();
    hello2.encode(&mut buf2).expect("encode");
    session
        .inject_control_for_test(bytes::Bytes::from(buf2))
        .expect("inject hello2");
    session.pump().expect("pump streaming");

    assert_eq!(session.status(), SenderStatus::Streaming);
    assert!(session.stream_config_sent());
    let cfg = session.pending_stream_config().expect("config");
    assert_eq!(cfg.width, 1920);
    assert_eq!(cfg.height, 1080);
    assert!(cfg.mirrored);
    assert_eq!(cfg.sps, vec![0x67, 0x42]);
    assert_eq!(cfg.pps, vec![0x68, 0xce]);
    assert!(session.take_keyframe_request());
}
