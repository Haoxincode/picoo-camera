use std::time::Duration;

use picoo_pairing::{TrustedDevice, TrustedDeviceStore};
use picoo_sender::SenderSession;
use picoo_session::ReceiverStatus;
use picoo_transport::{Endpoint, QuicSenderTransport};

use crate::{run_loopback_access_unit, run_paired_loopback_access_unit, ReceiverSession};

#[test]
fn loopback_sender_to_receiver_frame_hub() {
    let payload = b"test-access-unit";
    let frame = run_loopback_access_unit(payload).expect("loopback");
    assert_eq!(&frame.as_ref()[..payload.len()], payload);
}

#[test]
fn session_status_markers_cover_vcam_permission_and_network() {
    // REQ-PICOO-SESSION-001
    let mut receiver = ReceiverSession::new();
    assert_eq!(receiver.status(), ReceiverStatus::Disconnected);

    receiver.mark_permission_required();
    assert_eq!(receiver.status(), ReceiverStatus::PermissionRequired);

    receiver
        .listen(Endpoint {
            host: "127.0.0.1".into(),
            port: 0,
        })
        .expect("listen");
    assert_eq!(receiver.status(), ReceiverStatus::Discovering);

    receiver.mark_virtual_camera_unavailable();
    assert_eq!(
        receiver.status(),
        ReceiverStatus::VirtualCameraUnavailable
    );
    receiver.clear_virtual_camera_unavailable();
    assert_eq!(receiver.status(), ReceiverStatus::Discovering);

    // Network unstable only while live.
    receiver.mark_network_unstable();
    assert_eq!(receiver.status(), ReceiverStatus::Discovering);
}

#[test]
fn single_decode_per_access_unit_into_frame_hub() {
    // REQ-PICOO-MEDIA-006: one decode invocation per reassembled AU (hub fans out).
    let payload = b"single-decode-au";
    let mut receiver = ReceiverSession::new();
    receiver.set_jitter_target_ms(0);
    receiver.set_permit_unpaired_video(true);
    let bind = receiver
        .listen(Endpoint {
            host: "127.0.0.1".into(),
            port: 0,
        })
        .expect("listen");

    let mut sender = SenderSession::new(QuicSenderTransport::new());
    sender
        .connect(Endpoint {
            host: bind.ip().to_string(),
            port: bind.port(),
        })
        .expect("connect");

    for _ in 0..500 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if sender.is_connected() && receiver.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    sender
        .ingest_and_flush(payload, true, 1, 1)
        .expect("ingest");
    for _ in 0..200 {
        receiver.pump().expect("rx");
        sender.pump().ok();
        if receiver.stats().access_units > 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    let stats = receiver.stats();
    assert_eq!(stats.access_units, 1);
    assert_eq!(stats.decode_invocations, 1);
    assert!(receiver.latest_frame().is_some());
}

#[test]
fn paired_loopback_reaches_frame_hub_without_unpaired_bypass() {
    let payload = b"paired-product-path-au";
    let frame = run_paired_loopback_access_unit(payload).expect("paired loopback");
    assert_eq!(&frame.as_ref()[..payload.len()], payload);
}

#[test]
fn public_key_change_rejects_auto_connect() {
    let mut receiver = ReceiverSession::new();
    receiver.trusted_devices_mut().upsert(TrustedDevice {
        device_id: "android-sender".into(),
        device_name: "Pixel".into(),
        public_key: vec![1, 2, 3],
        certificate_fingerprint: "fp".into(),
        paired_at_ms: 0,
        last_connected_at_ms: None,
    });

    let bind = receiver
        .listen(Endpoint {
            host: "127.0.0.1".into(),
            port: 0,
        })
        .expect("listen");

    let mut sender = SenderSession::new(QuicSenderTransport::new());
    sender
        .connect(Endpoint {
            host: bind.ip().to_string(),
            port: bind.port(),
        })
        .expect("connect");

    for _ in 0..200 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        if sender.is_connected() && receiver.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    // Same device_id, different public key → pairing required again (PUC-007).
    sender
        .send_client_hello("android-sender", "Pixel", &[9, 9, 9])
        .expect("client hello");

    for _ in 0..100 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        if receiver.pairing_required() || receiver.pairing_short_code().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    assert!(receiver.pairing_required() || receiver.pairing_short_code().is_some());
    assert_ne!(receiver.status(), ReceiverStatus::Streaming);

    sender
        .ingest_and_flush(b"should-drop", true, 1, 1)
        .expect("send video");
    receiver.pump().expect("receiver pump");
    assert_eq!(receiver.stats().access_units, 0);
}

#[test]
fn default_placeholder_toggle_switches_waiting_frame() {
    // PRD §16: "默认占位画面" — branded waiting vs solid black.
    let mut receiver = ReceiverSession::new();
    receiver
        .publish_waiting_placeholder()
        .expect("branded placeholder");
    let branded = receiver.latest_frame().expect("frame").pixel_data.clone();
    assert!(branded.iter().any(|&b| b != 0 && b != 128));

    receiver.set_use_default_placeholder(false);
    receiver
        .publish_waiting_placeholder()
        .expect("black placeholder");
    let black = receiver.latest_frame().expect("frame").pixel_data.clone();
    let y_plane = &black[..1280 * 720];
    assert!(y_plane.iter().all(|&b| b == 0));
}

#[test]
fn unpaired_sender_video_is_dropped() {
    let mut receiver = ReceiverSession::new();
    let bind = receiver
        .listen(Endpoint {
            host: "127.0.0.1".into(),
            port: 0,
        })
        .expect("listen");

    let mut sender = SenderSession::new(QuicSenderTransport::new());
    sender
        .connect(Endpoint {
            host: bind.ip().to_string(),
            port: bind.port(),
        })
        .expect("connect");

    for _ in 0..200 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        if sender.is_connected() && receiver.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    sender
        .ingest_and_flush(b"blocked-au", true, 1, 1)
        .expect("send video");
    receiver.pump().expect("receiver pump");

    assert_eq!(receiver.stats().access_units, 0);
    assert!(receiver.stats().packets_dropped_unpaired > 0);
    assert_eq!(receiver.status(), ReceiverStatus::Connecting);
}

#[test]
fn paired_sender_enters_streaming_after_client_hello() {
    let mut receiver = ReceiverSession::new();
    receiver.trusted_devices_mut().upsert(TrustedDevice {
        device_id: "android-sender".into(),
        device_name: "Pixel".into(),
        public_key: vec![1, 2, 3],
        certificate_fingerprint: "fp".into(),
        paired_at_ms: 0,
        last_connected_at_ms: None,
    });

    let bind = receiver
        .listen(Endpoint {
            host: "127.0.0.1".into(),
            port: 0,
        })
        .expect("listen");

    let mut sender = SenderSession::new(QuicSenderTransport::new());
    sender
        .connect(Endpoint {
            host: bind.ip().to_string(),
            port: bind.port(),
        })
        .expect("connect");

    for _ in 0..200 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        if sender.is_connected() && receiver.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    sender
        .send_client_hello("android-sender", "Pixel Test", &[1, 2, 3])
        .expect("client hello");

    for _ in 0..100 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        if receiver.status() == ReceiverStatus::Streaming {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    assert_eq!(receiver.status(), ReceiverStatus::Streaming);
    assert!(!receiver.pairing_required());

    sender
        .ingest_and_flush(b"paired-au", true, 1, 1)
        .expect("send video");
    for _ in 0..100 {
        receiver.pump().expect("receiver pump");
        sender.pump().ok();
        if receiver.stats().access_units > 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(receiver.stats().access_units, 1);
}

#[test]
fn auto_accept_paired_off_requires_confirm_for_trusted_sender() {
    // PRD §16 / REQ-PICOO-UI-002: toggling off forces short-code even for trusted devices.
    let mut receiver = ReceiverSession::new();
    receiver.set_auto_accept_paired(false);
    receiver.trusted_devices_mut().upsert(TrustedDevice {
        device_id: "android-sender".into(),
        device_name: "Pixel".into(),
        public_key: vec![1, 2, 3],
        certificate_fingerprint: "fp".into(),
        paired_at_ms: 0,
        last_connected_at_ms: None,
    });

    let bind = receiver
        .listen(Endpoint {
            host: "127.0.0.1".into(),
            port: 0,
        })
        .expect("listen");

    let mut sender = SenderSession::new(QuicSenderTransport::new());
    sender
        .connect(Endpoint {
            host: bind.ip().to_string(),
            port: bind.port(),
        })
        .expect("connect");

    for _ in 0..200 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if sender.is_connected() && receiver.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    sender
        .send_client_hello("android-sender", "Pixel", &[1, 2, 3])
        .expect("hello");

    for _ in 0..100 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if receiver.pairing_short_code().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    assert_eq!(receiver.status(), ReceiverStatus::Pairing);
    assert!(receiver.pairing_short_code().is_some());
    assert_ne!(receiver.status(), ReceiverStatus::Streaming);

    sender
        .ingest_and_flush(b"blocked-until-confirm", true, 1, 1)
        .expect("send");
    receiver.pump().expect("rx");
    assert_eq!(receiver.stats().access_units, 0);
}

#[test]
fn first_time_pairing_flow_enables_video() {
    let identity = crate::ReceiverIdentity::default();
    let mut receiver = ReceiverSession::new().with_identity(identity.clone());

    let bind = receiver
        .listen(Endpoint {
            host: "127.0.0.1".into(),
            port: 0,
        })
        .expect("listen");

    let mut sender = SenderSession::new(QuicSenderTransport::new());
    sender
        .connect(Endpoint {
            host: bind.ip().to_string(),
            port: bind.port(),
        })
        .expect("connect");

    for _ in 0..200 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        if sender.is_connected() && receiver.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    sender
        .send_client_hello("new-phone", "Pixel 9", &[9, 9, 9])
        .expect("client hello");

    for _ in 0..100 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        if receiver.pairing_short_code().is_some() && sender.pairing_short_code().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    let recv_code = receiver.pairing_short_code().expect("receiver code");
    let send_code = sender.pairing_short_code().expect("sender code");
    assert_eq!(recv_code, send_code);
    assert_eq!(receiver.status(), ReceiverStatus::Pairing);

    receiver.confirm_pairing_locally();
    sender
        .send_pairing_confirm(&identity.receiver_id)
        .expect("pairing confirm");

    for _ in 0..100 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        if receiver.status() == ReceiverStatus::Streaming {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    assert_eq!(receiver.status(), ReceiverStatus::Streaming);
    assert!(receiver.trusted_devices().is_paired("new-phone"));

    sender
        .ingest_and_flush(b"paired-after-flow", true, 1, 1)
        .expect("send video");
    // Default jitter target is 50ms — pump until the AU is released (SESSION-002).
    for _ in 0..100 {
        receiver.pump().expect("receiver pump");
        sender.pump().ok();
        if receiver.stats().access_units > 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(receiver.stats().access_units, 1);
}

#[test]
fn receiver_sends_stats_to_paired_sender() {
    use picoo_sender::BitrateAction;
    use picoo_session::ReceiverStatus;

    let mut receiver = ReceiverSession::new();
    receiver.trusted_devices_mut().upsert(TrustedDevice {
        device_id: "android-sender".into(),
        device_name: "Pixel".into(),
        public_key: vec![1, 2, 3],
        certificate_fingerprint: "fp".into(),
        paired_at_ms: 0,
        last_connected_at_ms: None,
    });

    let bind = receiver
        .listen(Endpoint {
            host: "127.0.0.1".into(),
            port: 0,
        })
        .expect("listen");

    let mut sender = SenderSession::new(QuicSenderTransport::new());
    sender
        .connect(Endpoint {
            host: bind.ip().to_string(),
            port: bind.port(),
        })
        .expect("connect");

    for _ in 0..200 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        if sender.is_connected() && receiver.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    sender
        .send_client_hello("android-sender", "Pixel Test", &[1, 2, 3])
        .expect("client hello");

    for _ in 0..100 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        if receiver.status() == ReceiverStatus::Streaming {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    sender
        .ingest_and_flush(&[0u8; 1200], true, 1, 1)
        .expect("send video");
    // Release through the 50ms jitter buffer into FrameHub first.
    for _ in 0..100 {
        receiver.pump().expect("receiver pump");
        sender.pump().ok();
        if receiver.latest_frame().is_some_and(|f| f.timestamp_us > 0) {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        receiver
            .latest_frame()
            .is_some_and(|f| f.timestamp_us > 0),
        "expected decoded frame before stats interval"
    );

    std::thread::sleep(Duration::from_millis(1100));

    for _ in 0..20 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        if sender.last_receiver_stats().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    let stats = sender.last_receiver_stats().expect("receiver stats");
    assert!(stats.receive_bitrate > 0);
    // 1s stats interval with a static frame ⇒ high frame_age ⇒ bitrate decrease.
    assert!(stats.frame_age_ms > 200.0);
    // REQ-PICOO-PROTOCOL-006: RTT comes from transport link stats (loopback ≥ 0).
    assert!(stats.rtt_ms >= 0.0);
    assert!(stats.rtt_ms < 5_000.0);
    // Healthy loopback should not report pathological loss.
    assert!(stats.packet_loss < 0.5);
    assert_eq!(sender.last_bitrate_action(), BitrateAction::Decrease);
}

#[test]
fn stream_config_and_capabilities_after_paired_hello() {
    use picoo_sender::StreamConfigParams;
    use picoo_session::ReceiverStatus;

    let mut receiver = ReceiverSession::new();
    receiver.trusted_devices_mut().upsert(TrustedDevice {
        device_id: "android-sender".into(),
        device_name: "Pixel".into(),
        public_key: vec![1, 2, 3],
        certificate_fingerprint: "fp".into(),
        paired_at_ms: 0,
        last_connected_at_ms: None,
    });

    let bind = receiver
        .listen(Endpoint {
            host: "127.0.0.1".into(),
            port: 0,
        })
        .expect("listen");

    let mut sender = SenderSession::new(QuicSenderTransport::new());
    sender
        .connect(Endpoint {
            host: bind.ip().to_string(),
            port: bind.port(),
        })
        .expect("connect");

    for _ in 0..200 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        if sender.is_connected() && receiver.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    sender
        .send_client_hello("android-sender", "Pixel Test", &[1, 2, 3])
        .expect("client hello");

    for _ in 0..100 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        if receiver.status() == ReceiverStatus::Streaming {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    if receiver.stream_config().is_none() {
        sender
            .send_stream_config(&StreamConfigParams::default())
            .expect("stream config send");
    }

    for _ in 0..100 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        if receiver.stream_config().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    assert_eq!(receiver.status(), ReceiverStatus::Streaming);
    let config = receiver.stream_config().expect("stream config");
    assert_eq!(config.width, 1280);
    assert_eq!(config.height, 720);
    assert_eq!(config.stream_epoch, 1);
    assert!(sender.receiver_capabilities().is_some());
}

#[test]
fn trusted_store_persists_after_pairing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store_path = dir.path().join("trusted.json");

    let identity = crate::ReceiverIdentity::default();
    let mut receiver = ReceiverSession::new()
        .with_identity(identity.clone())
        .with_trusted_store(&store_path)
        .expect("load empty store");

    let bind = receiver
        .listen(Endpoint {
            host: "127.0.0.1".into(),
            port: 0,
        })
        .expect("listen");

    let mut sender = SenderSession::new(QuicSenderTransport::new());
    sender
        .connect(Endpoint {
            host: bind.ip().to_string(),
            port: bind.port(),
        })
        .expect("connect");

    for _ in 0..200 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        if sender.is_connected() && receiver.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    sender
        .send_client_hello("persist-phone", "Pixel", &[7, 7, 7])
        .expect("client hello");

    for _ in 0..100 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        if receiver.pairing_short_code().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    receiver.confirm_pairing_locally();
    sender
        .send_pairing_confirm(&identity.receiver_id)
        .expect("pairing confirm");

    for _ in 0..100 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        if receiver.status() == ReceiverStatus::Streaming {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    assert!(store_path.exists());
    let loaded = TrustedDeviceStore::load_from_path(&store_path).expect("reload store");
    assert!(loaded.is_paired("persist-phone"));
    assert_eq!(
        loaded.get("persist-phone").map(|d| d.public_key.as_slice()),
        Some([7u8, 7, 7].as_slice())
    );
}

#[test]
fn remove_trusted_device_requires_repair() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store_path = dir.path().join("trusted.json");

    let mut store = TrustedDeviceStore::new();
    store.upsert(TrustedDevice {
        device_id: "phone-1".into(),
        device_name: "Pixel".into(),
        public_key: vec![1, 2, 3],
        certificate_fingerprint: "fp".into(),
        paired_at_ms: 0,
        last_connected_at_ms: None,
    });
    store.save_to_path(&store_path).expect("save");

    let mut receiver = ReceiverSession::new()
        .with_trusted_store(&store_path)
        .expect("load store");

    assert!(receiver.remove_trusted_device("phone-1").expect("remove"));

    let loaded = TrustedDeviceStore::load_from_path(&store_path).expect("reload");
    assert!(!loaded.is_paired("phone-1"));
}

#[test]
fn disconnect_holds_last_frame_then_shows_placeholder() {
    use crate::ReceiverIdentity;

    let identity = ReceiverIdentity::default();
    let mut receiver = ReceiverSession::new().with_identity(identity.clone());
    receiver.set_last_frame_hold_for_test(Duration::from_millis(60));
    let bind = receiver
        .listen(Endpoint {
            host: "127.0.0.1".into(),
            port: 0,
        })
        .expect("listen");

    let mut sender = SenderSession::new(QuicSenderTransport::new());
    sender
        .connect(Endpoint {
            host: bind.ip().to_string(),
            port: bind.port(),
        })
        .expect("connect");

    for _ in 0..200 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if sender.is_connected() && receiver.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    sender
        .send_client_hello("hold-phone", "Hold Phone", &[3, 3, 3])
        .expect("hello");
    for _ in 0..100 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if receiver.pairing_short_code().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    receiver.confirm_pairing_locally();
    sender
        .send_pairing_confirm(&identity.receiver_id)
        .expect("confirm");
    for _ in 0..100 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if receiver.status() == ReceiverStatus::Streaming {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(receiver.status(), ReceiverStatus::Streaming);

    sender
        .ingest_and_flush(b"live-frame-before-disconnect", true, 1, 1)
        .expect("ingest");
    for _ in 0..100 {
        receiver.pump().expect("rx");
        sender.pump().ok();
        if receiver.latest_frame().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let live_ts = receiver
        .latest_frame()
        .expect("live frame")
        .timestamp_us;
    assert!(live_ts > 0);

    receiver.inject_peer_disconnect_for_test();
    assert_eq!(receiver.status(), ReceiverStatus::Reconnecting);
    assert_eq!(
        receiver.latest_frame().expect("held frame").timestamp_us,
        live_ts
    );

    std::thread::sleep(Duration::from_millis(80));
    receiver.pump().expect("finalize hold");
    assert_eq!(receiver.status(), ReceiverStatus::Discovering);
    assert_eq!(
        receiver.latest_frame().expect("placeholder").timestamp_us,
        0
    );
}

#[test]
fn default_jitter_holds_au_until_target_delay() {
    // REQ-PICOO-SESSION-002: default 50ms target delays decode until media clock catches up.
    let mut receiver = ReceiverSession::new();
    receiver.trusted_devices_mut().upsert(TrustedDevice {
        device_id: "android-sender".into(),
        device_name: "Pixel".into(),
        public_key: vec![1, 2, 3],
        certificate_fingerprint: "fp".into(),
        paired_at_ms: 0,
        last_connected_at_ms: None,
    });

    let bind = receiver
        .listen(Endpoint {
            host: "127.0.0.1".into(),
            port: 0,
        })
        .expect("listen");

    let mut sender = SenderSession::new(QuicSenderTransport::new());
    sender
        .connect(Endpoint {
            host: bind.ip().to_string(),
            port: bind.port(),
        })
        .expect("connect");

    for _ in 0..200 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if sender.is_connected() && receiver.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    sender
        .send_client_hello("android-sender", "Pixel", &[1, 2, 3])
        .expect("hello");
    for _ in 0..100 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if receiver.status() == ReceiverStatus::Streaming {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    sender
        .ingest_and_flush(b"jitter-hold-au", true, 1, 1)
        .expect("ingest");
    receiver.pump().expect("rx");
    // Immediately after first pump the AU should still be in the jitter buffer.
    assert_eq!(receiver.stats().access_units, 0);

    let started = std::time::Instant::now();
    while started.elapsed() < Duration::from_millis(200) {
        receiver.pump().expect("rx");
        if receiver.stats().access_units > 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(receiver.stats().access_units, 1);
    assert!(
        started.elapsed() >= Duration::from_millis(40),
        "expected ~50ms hold, released too early: {:?}",
        started.elapsed()
    );
}

/// REQ-PICOO-SESSION-005 — paired loopback soak (default 60s; set `PICOO_SOAK_SECONDS`).
///
/// Run: `PICOO_SOAK_SECONDS=60 cargo test -p picoo-receiver --lib soak_paired_loopback_memory_stable -- --ignored --nocapture`
#[test]
#[ignore = "long-running soak; enable via --ignored"]
fn soak_paired_loopback_memory_stable() {
    let soak_secs: u64 = std::env::var("PICOO_SOAK_SECONDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    let sample_every: u64 = std::env::var("PICOO_SOAK_SAMPLE_EVERY")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);

    let identity = crate::ReceiverIdentity::default();
    let mut receiver = ReceiverSession::new().with_identity(identity.clone());
    receiver.set_jitter_target_ms(0);
    let bind = receiver
        .listen(Endpoint {
            host: "127.0.0.1".into(),
            port: 0,
        })
        .expect("listen");

    let mut sender = SenderSession::new(QuicSenderTransport::new());
    sender
        .connect(Endpoint {
            host: bind.ip().to_string(),
            port: bind.port(),
        })
        .expect("connect");

    for _ in 0..500 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if sender.is_connected() && receiver.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(receiver.is_connected());

    sender
        .send_client_hello("soak-phone", "Soak Phone", &[7, 7, 7])
        .expect("hello");
    for _ in 0..200 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if receiver.pairing_short_code().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    receiver.confirm_pairing_locally();
    sender
        .send_pairing_confirm(&identity.receiver_id)
        .expect("confirm");
    for _ in 0..200 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if receiver.status() == ReceiverStatus::Streaming {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(receiver.status(), ReceiverStatus::Streaming);

    let deadline = std::time::Instant::now() + Duration::from_secs(soak_secs);
    let start = std::time::Instant::now();
    let mut next_sample = std::time::Instant::now();
    let mut samples: Vec<(u64, u64, u64)> = Vec::new(); // (elapsed_s, au, rss_kb)
    let mut frame_id = 1u64;

    while std::time::Instant::now() < deadline {
        let payload = format!("soak-frame-{frame_id}");
        sender
            .ingest_and_flush(payload.as_bytes(), frame_id % 30 == 1, frame_id, 1)
            .expect("ingest");
        frame_id += 1;
        for _ in 0..8 {
            receiver.pump().expect("rx");
            sender.pump().ok();
        }
        if std::time::Instant::now() >= next_sample {
            let elapsed = start.elapsed().as_secs();
            let rss = linux_vm_rss_kb().unwrap_or(0);
            let au = receiver.stats().access_units;
            eprintln!("soak sample elapsed={elapsed}s au={au} rss_kb={rss}");
            samples.push((elapsed, au, rss));
            next_sample += Duration::from_secs(sample_every);
        }
        std::thread::sleep(Duration::from_millis(5));
    }

    assert!(
        receiver.stats().access_units > 10,
        "expected many AUs during soak, got {}",
        receiver.stats().access_units
    );
    if samples.len() >= 3 {
        let first = samples[0].2;
        let last = samples[samples.len() - 1].2;
        // Soft bound: RSS should not grow unboundedly (allow 64 MiB headroom for allocator).
        assert!(
            last <= first.saturating_add(64 * 1024),
            "RSS grew too much during soak: first={first}kb last={last}kb samples={samples:?}"
        );
    }
}

fn linux_vm_rss_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            let kb = rest.split_whitespace().next()?.parse().ok()?;
            return Some(kb);
        }
    }
    None
}
