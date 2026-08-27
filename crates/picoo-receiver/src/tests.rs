use std::time::Duration;

use picoo_pairing::{TrustedDevice, TrustedDeviceStore};
use picoo_sender::SenderSession;
use picoo_session::ReceiverStatus;
use picoo_transport::{Endpoint, QuicSenderTransport};

use crate::{run_loopback_access_unit, ReceiverSession};

#[test]
fn loopback_sender_to_receiver_frame_hub() {
    let payload = b"test-access-unit";
    let frame = run_loopback_access_unit(payload).expect("loopback");
    assert_eq!(&frame.as_ref()[..payload.len()], payload);
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
    receiver.pump().expect("receiver pump");
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
    receiver.pump().expect("receiver pump");

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
