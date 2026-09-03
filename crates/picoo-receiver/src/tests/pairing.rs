use std::time::{Duration, Instant};

use picoo_pairing::{TrustedDevice, TrustedDeviceStore};
use picoo_sender::SenderSession;
use picoo_session::{ReceiverStatus, SenderStatus};
use picoo_transport::{Endpoint, QuicSenderTransport};

use crate::{ReceiverSession, PAIRING_CHALLENGE_TTL};

use super::{pump_pair_for, use_stub_decoder};

#[test]
fn public_key_change_rejects_auto_connect() {
    // REQ-PICOO-PAIRING-004: same device_id + different public key → hard reject
    // (SessionError PUBLIC_KEY_CHANGED), trust entry unchanged, no pending re-pair.
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
        .send_client_hello("android-sender", "Pixel", &[9, 9, 9])
        .expect("client hello");

    for _ in 0..100 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        if sender.last_session_error() == Some("PUBLIC_KEY_CHANGED") {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    assert_eq!(sender.last_session_error(), Some("PUBLIC_KEY_CHANGED"));
    assert!(!receiver.pairing_required());
    assert!(receiver.pairing_short_code().is_none());
    assert_ne!(receiver.status(), ReceiverStatus::Streaming);
    assert!(receiver.trusted_devices().is_paired("android-sender"));
    assert!(receiver
        .trusted_devices()
        .verify_paired_key("android-sender", &[1, 2, 3])
        .is_ok());

    // Video must not reach FrameHub after key-mismatch reject.
    let _ = sender.ingest_and_flush(b"should-drop", true, 1, 1);
    let _ = receiver.pump();
    assert_eq!(receiver.stats().access_units, 0);
}

#[test]
fn pairing_challenge_expires_clears_short_code() {
    // AC-M-PAIR-02 / PUC-001: pending pairing TTL (60s) — late confirm must fail.
    use crate::ReceiverIdentity;

    let identity = ReceiverIdentity::default();
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
        receiver.pump().ok();
        sender.pump().ok();
        if sender.is_connected() && receiver.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    sender
        .send_client_hello("ttl-phone", "TTL", &[4, 4, 4])
        .expect("hello");
    for _ in 0..100 {
        receiver.pump().ok();
        sender.pump().ok();
        if receiver.pairing_short_code().is_some() && sender.pairing_short_code().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(receiver.pairing_short_code().is_some());
    assert!(receiver
        .pairing_ttl_remaining()
        .is_some_and(|d| d <= PAIRING_CHALLENGE_TTL));

    receiver.force_expire_pending_pairing_for_test();
    assert!(receiver.pairing_short_code().is_none());
    assert!(!receiver.is_awaiting_pairing_confirm());

    // Late confirm after expiry must not begin streaming.
    receiver
        .confirm_pairing_locally()
        .expect("desktop confirm after expiry");
    let _ = sender.send_pairing_confirm(&identity.receiver_id);
    for _ in 0..40 {
        receiver.pump().ok();
        sender.pump().ok();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_ne!(receiver.status(), ReceiverStatus::Streaming);
}

#[test]
fn desktop_reject_sends_explicit_pairing_rejected() {
    // REQ-PICOO-PAIRING-001 / AC-M-PAIR-03: an explicit desktop reject is
    // distinguishable from an unrelated disconnect on the mobile side.
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
    pump_pair_for(&mut receiver, &mut sender, Duration::from_millis(100));

    sender
        .send_client_hello("reject-phone", "Reject Phone", &[7, 7, 7])
        .expect("hello");
    for _ in 0..100 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        if receiver.pairing_short_code().is_some() && sender.pairing_short_code().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(receiver.is_awaiting_pairing_confirm());

    receiver
        .reject_pairing_locally()
        .expect("desktop reject pairing");
    for _ in 0..100 {
        sender.pump().expect("sender pump");
        if sender.last_session_error() == Some("PAIRING_REJECTED") {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    assert_eq!(sender.last_session_error(), Some("PAIRING_REJECTED"));
    assert!(!receiver.is_awaiting_pairing_confirm());
    assert!(receiver.pairing_short_code().is_none());
    assert_ne!(receiver.status(), ReceiverStatus::Streaming);
}

#[test]
fn paired_sender_enters_streaming_after_client_hello() {
    let mut receiver = ReceiverSession::new();
    use_stub_decoder(&mut receiver);
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
        if receiver.status() == ReceiverStatus::Streaming
            && sender.status() == SenderStatus::Streaming
        {
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

    let err = sender
        .ingest_and_flush(b"blocked-until-confirm", true, 1, 1)
        .expect_err("sender must block media until pairing commits");
    assert!(matches!(err, picoo_sender::SenderError::MediaNotReady));
    receiver.pump().expect("rx");
    assert_eq!(receiver.stats().access_units, 0);
}

#[test]
fn pairing_confirm_false_positive_does_not_complete_pairing() {
    // REQ-PICOO-PAIRING-002 / device-e2e §C: prost may decode unrelated blobs as
    // PairingConfirm — receiver must verify signature before completing pairing.
    use picoo_protocol::control::PairingConfirm;
    use prost::Message;

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
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if sender.is_connected() && receiver.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    sender
        .send_client_hello("flaky-phone", "Pixel", &[7, 7, 7])
        .expect("hello");
    for _ in 0..100 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if receiver.pairing_short_code().is_some() && sender.pairing_short_code().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    assert_eq!(receiver.status(), ReceiverStatus::Pairing);
    receiver.confirm_pairing_locally().expect("desktop confirm");

    let bogus = PairingConfirm {
        confirm_signature: vec![0u8; 32],
    };
    let mut buf = Vec::new();
    bogus.encode(&mut buf).expect("encode bogus confirm");
    receiver
        .inject_control_for_test(bytes::Bytes::from(buf))
        .expect("inject bogus confirm");
    assert_eq!(receiver.status(), ReceiverStatus::Pairing);
    assert!(receiver.pairing_short_code().is_some());

    sender
        .send_pairing_confirm(&identity.receiver_id)
        .expect("real confirm");
    for _ in 0..100 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if receiver.status() == ReceiverStatus::Streaming {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(receiver.status(), ReceiverStatus::Streaming);
}

#[test]
fn phone_confirm_before_desktop_confirm_completes_without_retry() {
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
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if sender.is_connected() && receiver.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    sender
        .send_client_hello("early-phone", "Pixel", &[8, 8, 8])
        .expect("hello");
    for _ in 0..100 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if receiver.pairing_short_code().is_some() && sender.pairing_short_code().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    assert_eq!(receiver.status(), ReceiverStatus::Pairing);
    sender
        .send_pairing_confirm(&identity.receiver_id)
        .expect("early confirm");
    pump_pair_for(&mut receiver, &mut sender, Duration::from_millis(200));
    assert_eq!(receiver.status(), ReceiverStatus::Pairing);
    assert_eq!(sender.status(), SenderStatus::Pairing);
    assert!(!receiver.trusted_devices().is_paired("early-phone"));
    assert!(!sender.trusted_devices().is_paired(&identity.receiver_id));

    receiver.confirm_pairing_locally().expect("desktop confirm");
    let streaming_deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < streaming_deadline {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if receiver.status() == ReceiverStatus::Streaming
            && sender.status() == SenderStatus::Streaming
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(receiver.status(), ReceiverStatus::Streaming);
    assert_eq!(sender.status(), SenderStatus::Streaming);
    assert!(receiver.trusted_devices().is_paired("early-phone"));
    assert!(sender.trusted_devices().is_paired(&identity.receiver_id));
}

#[test]
fn first_time_pairing_flow_enables_video() {
    let identity = crate::ReceiverIdentity::default();
    let mut receiver = ReceiverSession::new().with_identity(identity.clone());
    use_stub_decoder(&mut receiver);

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

    receiver.confirm_pairing_locally().expect("desktop confirm");
    sender
        .send_pairing_confirm(&identity.receiver_id)
        .expect("pairing confirm");

    for _ in 0..100 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        if receiver.status() == ReceiverStatus::Streaming
            && sender.status() == SenderStatus::Streaming
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    assert_eq!(receiver.status(), ReceiverStatus::Streaming);
    assert_eq!(sender.status(), SenderStatus::Streaming);
    assert!(receiver.trusted_devices().is_paired("new-phone"));
    assert!(sender.trusted_devices().is_paired(&identity.receiver_id));

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
        if receiver.pairing_short_code().is_some() && sender.pairing_short_code().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    receiver.confirm_pairing_locally().expect("desktop confirm");
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
fn clear_trusted_devices_requires_repair() {
    // REQ-PICOO-PAIRING-005 / PUC-007 — wipe all pairings.
    let dir = tempfile::tempdir().expect("tempdir");
    let store_path = dir.path().join("trusted.json");

    let mut store = TrustedDeviceStore::new();
    store.upsert(TrustedDevice {
        device_id: "phone-a".into(),
        device_name: "A".into(),
        public_key: vec![1],
        certificate_fingerprint: "a".into(),
        paired_at_ms: 0,
        last_connected_at_ms: None,
    });
    store.upsert(TrustedDevice {
        device_id: "phone-b".into(),
        device_name: "B".into(),
        public_key: vec![2],
        certificate_fingerprint: "b".into(),
        paired_at_ms: 0,
        last_connected_at_ms: None,
    });
    store.save_to_path(&store_path).expect("save");

    let mut receiver = ReceiverSession::new()
        .with_trusted_store(&store_path)
        .expect("load store");
    assert_eq!(receiver.clear_trusted_devices().expect("clear"), 2);

    let loaded = TrustedDeviceStore::load_from_path(&store_path).expect("reload");
    assert!(loaded.is_empty());
    assert!(!loaded.is_paired("phone-a"));
    assert!(!loaded.is_paired("phone-b"));
}

#[test]
fn newly_paired_identity_can_replace_same_name_history() {
    // REQ-PICOO-PAIRING-006 — a previously unknown identity emits one exact
    // cleanup decision only after the full pairing commit.
    let dir = tempfile::tempdir().expect("tempdir");
    let store_path = dir.path().join("trusted.json");
    let mut store = TrustedDeviceStore::new();
    for (device_id, device_name, key) in [
        ("phone-old-a", "pixel 9 pro", vec![1]),
        ("phone-old-b", " Pixel 9 Pro ", vec![2]),
        ("other-phone", "iPhone", vec![4]),
    ] {
        store.upsert(TrustedDevice {
            device_id: device_id.into(),
            device_name: device_name.into(),
            public_key: key,
            certificate_fingerprint: device_id.into(),
            paired_at_ms: 0,
            last_connected_at_ms: None,
        });
    }
    store.save_to_path(&store_path).expect("save");

    let identity = crate::ReceiverIdentity::default();
    let mut receiver = ReceiverSession::new()
        .with_identity(identity.clone())
        .with_trusted_store(&store_path)
        .expect("load store");
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
    pump_pair_for(&mut receiver, &mut sender, Duration::from_millis(100));
    sender
        .send_client_hello("phone-current", "Pixel 9 Pro", &[3])
        .expect("hello");
    for _ in 0..100 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        if receiver.pairing_short_code().is_some() && sender.pairing_short_code().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    receiver.confirm_pairing_locally().expect("desktop confirm");
    sender
        .send_pairing_confirm(&identity.receiver_id)
        .expect("sender confirm");
    for _ in 0..100 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        if receiver.status() == ReceiverStatus::Streaming {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    let replacement = receiver
        .trusted_identity_replacement()
        .cloned()
        .expect("post-pairing decision");
    assert_eq!(replacement.device_name, "Pixel 9 Pro");
    assert_eq!(
        replacement
            .previous_identities
            .iter()
            .map(|candidate| candidate.device_id.as_str())
            .collect::<Vec<_>>(),
        vec!["phone-old-a", "phone-old-b"]
    );

    // The immutable consent snapshot is part of the same durable trust update.
    // A process restart restores it without treating a trusted reconnect as a
    // fresh name-based cleanup decision.
    drop(receiver);
    let mut receiver = ReceiverSession::new()
        .with_trusted_store(&store_path)
        .expect("reload receiver store");
    assert_eq!(receiver.trusted_identity_replacement(), Some(&replacement));

    // A later same-name record was never shown by this decision and must not
    // be widened into the user's confirmed deletion set.
    receiver.trusted_devices_mut().upsert(TrustedDevice {
        device_id: "phone-late".into(),
        device_name: "Pixel 9 Pro".into(),
        public_key: vec![9],
        certificate_fingerprint: "late".into(),
        paired_at_ms: 1,
        last_connected_at_ms: Some(1),
    });
    assert_eq!(
        receiver
            .replace_trusted_identity_history(replacement.revision)
            .expect("replace"),
        2
    );
    let loaded = TrustedDeviceStore::load_from_path(&store_path).expect("reload");
    assert!(loaded.is_paired("phone-current"));
    assert!(loaded.is_paired("other-phone"));
    assert!(loaded.is_paired("phone-late"));
    assert!(!loaded.is_paired("phone-old-a"));
    assert!(!loaded.is_paired("phone-old-b"));
    assert!(receiver.trusted_identity_replacement().is_none());
}

#[test]
fn trusted_reconnect_never_emits_identity_replacement_decision() {
    let mut receiver = ReceiverSession::new();
    for (device_id, key) in [("phone-a", vec![1]), ("phone-b", vec![2])] {
        receiver.trusted_devices_mut().upsert(TrustedDevice {
            device_id: device_id.into(),
            device_name: "Pixel".into(),
            public_key: key.clone(),
            certificate_fingerprint: device_id.into(),
            paired_at_ms: 0,
            last_connected_at_ms: None,
        });
    }
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
    pump_pair_for(&mut receiver, &mut sender, Duration::from_millis(100));
    sender
        .send_client_hello("phone-a", "Pixel", &[1])
        .expect("hello");
    for _ in 0..100 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        if receiver.status() == ReceiverStatus::Streaming {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(receiver.status(), ReceiverStatus::Streaming);
    assert!(receiver.trusted_identity_replacement().is_none());
}

#[test]
fn keeping_same_name_identities_consumes_only_the_current_decision() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store_path = dir.path().join("trusted.json");
    let mut receiver = ReceiverSession::new()
        .with_trusted_store(&store_path)
        .expect("load store");
    for device_id in ["phone-current", "phone-old"] {
        receiver.trusted_devices_mut().upsert(TrustedDevice {
            device_id: device_id.into(),
            device_name: "Pixel".into(),
            public_key: vec![device_id.len() as u8],
            certificate_fingerprint: device_id.into(),
            paired_at_ms: 0,
            last_connected_at_ms: None,
        });
    }
    receiver.prepare_trusted_identity_replacement("phone-current");
    receiver
        .trusted_devices()
        .save_to_path(&store_path)
        .expect("persist decision");
    let revision = receiver
        .trusted_identity_replacement()
        .expect("decision")
        .revision;
    assert!(receiver
        .dismiss_trusted_identity_replacement(revision)
        .expect("dismiss decision"));
    assert!(receiver.trusted_identity_replacement().is_none());
    assert!(!receiver
        .dismiss_trusted_identity_replacement(revision)
        .expect("decision already dismissed"));
    assert!(receiver.trusted_devices().is_paired("phone-current"));
    assert!(receiver.trusted_devices().is_paired("phone-old"));

    let reloaded = ReceiverSession::new()
        .with_trusted_store(&store_path)
        .expect("reload store");
    assert!(reloaded.trusted_identity_replacement().is_none());
}

#[test]
fn failed_identity_history_replace_rolls_back_memory() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store_path = dir.path().join("trusted.json");
    let mut store = TrustedDeviceStore::new();
    store.upsert(TrustedDevice {
        device_id: "phone-current".into(),
        device_name: "Pixel".into(),
        public_key: vec![2],
        certificate_fingerprint: "current".into(),
        paired_at_ms: 0,
        last_connected_at_ms: None,
    });
    store.upsert(TrustedDevice {
        device_id: "phone-old".into(),
        device_name: "Pixel".into(),
        public_key: vec![1],
        certificate_fingerprint: "old".into(),
        paired_at_ms: 0,
        last_connected_at_ms: None,
    });
    store.save_to_path(&store_path).expect("save");
    let mut receiver = ReceiverSession::new()
        .with_trusted_store(&store_path)
        .expect("load store");
    receiver.prepare_trusted_identity_replacement("phone-current");
    let revision = receiver
        .trusted_identity_replacement()
        .expect("decision")
        .revision;
    // Replace the loaded file with a directory so the next atomic persistence
    // fails after the in-memory removal has begun.
    std::fs::remove_file(&store_path).expect("remove store file");
    std::fs::create_dir(&store_path).expect("directory target");

    assert!(receiver.replace_trusted_identity_history(revision).is_err());
    assert!(receiver.trusted_devices().is_paired("phone-current"));
    assert!(receiver.trusted_devices().is_paired("phone-old"));
}
