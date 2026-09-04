use std::time::Duration;

use picoo_pairing::{TrustedDevice, TrustedDeviceStore};
use picoo_sender::SenderSession;
use picoo_session::ReceiverStatus;
use picoo_transport::{Endpoint, QuicSenderTransport};

use crate::ReceiverSession;

use super::pump_pair_for;

fn test_trusted_device(device_name: &str, secret_byte: u8) -> TrustedDevice {
    let identity =
        picoo_pairing::DeviceIdentity::from_secret_bytes(device_name, &[secret_byte; 32])
            .expect("test identity");
    picoo_pairing::trusted_device_from_pairing(
        identity.device_id(),
        identity.device_name(),
        identity.public_key(),
        0,
    )
}

#[test]
fn remove_trusted_device_requires_repair() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store_path = dir.path().join("trusted.json");

    let mut store = TrustedDeviceStore::new();
    let device = test_trusted_device("Pixel", 1);
    let device_id = device.device_id.clone();
    store.upsert(device);
    store.save_to_path(&store_path).expect("save");

    let mut receiver = ReceiverSession::new()
        .with_trusted_store(&store_path)
        .expect("load store");

    assert!(receiver.remove_trusted_device(&device_id).expect("remove"));

    let loaded = TrustedDeviceStore::load_from_path(&store_path).expect("reload");
    assert!(!loaded.is_paired(&device_id));
}

#[test]
fn clear_trusted_devices_requires_repair() {
    // REQ-PICOO-PAIRING-005 / PUC-007 — wipe all pairings.
    let dir = tempfile::tempdir().expect("tempdir");
    let store_path = dir.path().join("trusted.json");

    let mut store = TrustedDeviceStore::new();
    let first = test_trusted_device("A", 2);
    let second = test_trusted_device("B", 3);
    let first_id = first.device_id.clone();
    let second_id = second.device_id.clone();
    store.upsert(first);
    store.upsert(second);
    store.save_to_path(&store_path).expect("save");

    let mut receiver = ReceiverSession::new()
        .with_trusted_store(&store_path)
        .expect("load store");
    assert_eq!(receiver.clear_trusted_devices().expect("clear"), 2);

    let loaded = TrustedDeviceStore::load_from_path(&store_path).expect("reload");
    assert!(loaded.is_empty());
    assert!(!loaded.is_paired(&first_id));
    assert!(!loaded.is_paired(&second_id));
}

#[test]
fn newly_paired_identity_can_replace_same_name_history() {
    // REQ-PICOO-PAIRING-006 — a previously unknown identity emits one exact
    // cleanup decision only after the full pairing commit.
    let dir = tempfile::tempdir().expect("tempdir");
    let store_path = dir.path().join("trusted.json");
    let mut store = TrustedDeviceStore::new();
    let old_a = test_trusted_device("pixel 9 pro", 4);
    let old_b = test_trusted_device(" Pixel 9 Pro ", 5);
    let other = test_trusted_device("iPhone", 6);
    let mut old_ids = [old_a.device_id.clone(), old_b.device_id.clone()];
    old_ids.sort();
    let other_id = other.device_id.clone();
    store.upsert(old_a);
    store.upsert(old_b);
    store.upsert(other);
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
    let sender_identity =
        picoo_pairing::DeviceIdentity::generate("Pixel 9 Pro").expect("sender identity");
    let current_id = sender_identity.device_id().to_owned();
    let mut sender = SenderSession::new_with_identity(QuicSenderTransport::new(), sender_identity);
    sender
        .connect(Endpoint {
            host: bind.ip().to_string(),
            port: bind.port(),
        })
        .expect("connect");
    pump_pair_for(&mut receiver, &mut sender, Duration::from_millis(100));
    sender.send_client_hello().expect("hello");
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
        .send_pairing_confirm(identity.receiver_id())
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
        old_ids.iter().map(String::as_str).collect::<Vec<_>>()
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
    let late = test_trusted_device("Pixel 9 Pro", 7);
    let late_id = late.device_id.clone();
    receiver.trusted_devices_mut().upsert(late);
    assert_eq!(
        receiver
            .replace_trusted_identity_history(replacement.revision)
            .expect("replace"),
        2
    );
    let loaded = TrustedDeviceStore::load_from_path(&store_path).expect("reload");
    assert!(loaded.is_paired(&current_id));
    assert!(loaded.is_paired(&other_id));
    assert!(loaded.is_paired(&late_id));
    assert!(old_ids.iter().all(|device_id| !loaded.is_paired(device_id)));
    assert!(receiver.trusted_identity_replacement().is_none());
}

#[test]
fn trusted_reconnect_never_emits_identity_replacement_decision() {
    let mut receiver = ReceiverSession::new();
    receiver.trusted_devices_mut().upsert(TrustedDevice {
        device_id: "phone-history".into(),
        device_name: "Picoo Test Sender".into(),
        public_key: vec![2],
        certificate_fingerprint: "history".into(),
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
    super::trust_receiver(&mut sender, &mut receiver);
    sender
        .connect(Endpoint {
            host: bind.ip().to_string(),
            port: bind.port(),
        })
        .expect("connect");
    pump_pair_for(&mut receiver, &mut sender, Duration::from_millis(100));
    sender.send_client_hello().expect("hello");
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
    let current = test_trusted_device("Pixel", 8);
    let previous = test_trusted_device("Pixel", 9);
    let current_id = current.device_id.clone();
    let previous_id = previous.device_id.clone();
    receiver.trusted_devices_mut().upsert(current);
    receiver.trusted_devices_mut().upsert(previous);
    receiver.prepare_trusted_identity_replacement(&current_id);
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
    assert!(receiver.trusted_devices().is_paired(&current_id));
    assert!(receiver.trusted_devices().is_paired(&previous_id));

    let reloaded = ReceiverSession::new()
        .with_trusted_store(&store_path)
        .expect("reload store");
    assert!(reloaded.trusted_identity_replacement().is_none());
}

#[test]
fn changed_identity_replacement_candidate_rejects_stale_consent() {
    let mut receiver = ReceiverSession::new();
    let current = test_trusted_device("Pixel", 12);
    let previous = test_trusted_device("Pixel", 13);
    let current_id = current.device_id.clone();
    let previous_id = previous.device_id.clone();
    receiver.trusted_devices_mut().upsert(current);
    receiver.trusted_devices_mut().upsert(previous);
    receiver.prepare_trusted_identity_replacement(&current_id);
    let revision = receiver
        .trusted_identity_replacement()
        .expect("decision")
        .revision;

    receiver
        .trusted_devices_mut()
        .touch_last_connected(&previous_id, 42);

    assert!(matches!(
        receiver.replace_trusted_identity_history(revision),
        Err(crate::ReceiverError::StaleTrustedIdentityReplacement)
    ));
    assert!(receiver.trusted_devices().is_paired(&current_id));
    assert!(receiver.trusted_devices().is_paired(&previous_id));
    assert_eq!(
        receiver
            .trusted_identity_replacement()
            .map(|replacement| replacement.revision),
        Some(revision)
    );
}

#[test]
fn failed_identity_history_replace_rolls_back_memory() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store_path = dir.path().join("trusted.json");
    let mut store = TrustedDeviceStore::new();
    let current = test_trusted_device("Pixel", 10);
    let previous = test_trusted_device("Pixel", 11);
    let current_id = current.device_id.clone();
    let previous_id = previous.device_id.clone();
    store.upsert(current);
    store.upsert(previous);
    store.save_to_path(&store_path).expect("save");
    let mut receiver = ReceiverSession::new()
        .with_trusted_store(&store_path)
        .expect("load store");
    receiver.prepare_trusted_identity_replacement(&current_id);
    let revision = receiver
        .trusted_identity_replacement()
        .expect("decision")
        .revision;
    // Replace the loaded file with a directory so the next atomic persistence
    // fails after the in-memory removal has begun.
    std::fs::remove_file(&store_path).expect("remove store file");
    std::fs::create_dir(&store_path).expect("directory target");

    assert!(receiver.replace_trusted_identity_history(revision).is_err());
    assert!(receiver.trusted_devices().is_paired(&current_id));
    assert!(receiver.trusted_devices().is_paired(&previous_id));
}
