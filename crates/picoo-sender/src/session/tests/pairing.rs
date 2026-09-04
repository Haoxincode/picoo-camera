use picoo_protocol::control::{PairingApproval, PairingComplete};

use super::super::pairing::{PAIRING_APPROVAL_PHASE, PAIRING_COMPLETE_PHASE};
use super::*;

#[test]
fn pairing_confirm_waits_for_receiver_completion() {
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
    session.send_client_hello().expect("client hello");

    let receiver = picoo_pairing::DeviceIdentity::generate("Picoo Camera").expect("identity");
    let (hello, transcript_hash) = signed_server_hello(&session, &receiver, true);
    session
        .inject_control_payload_for_test(ControlPayload::ServerHello(hello))
        .expect("inject hello");
    assert!(session.pairing_short_code().is_some());
    let _ = session.take_keyframe_request();

    let approval = PairingApproval {
        transcript_hash: transcript_hash.to_vec(),
        identity_signature: picoo_pairing::sign_transcript_phase(
            &receiver,
            &transcript_hash,
            PAIRING_APPROVAL_PHASE,
        )
        .to_vec(),
    };
    session
        .inject_control_payload_for_test(ControlPayload::PairingApproval(approval.clone()))
        .expect("inject premature approval");
    assert_eq!(session.status(), SenderStatus::Pairing);
    assert_eq!(
        session.last_session_error(),
        Some("PAIRING_LOCAL_CONFIRM_MISSING")
    );
    assert!(!session.trusted_devices().is_paired(receiver.device_id()));

    session
        .send_pairing_confirm(receiver.device_id())
        .expect("confirm");

    assert_eq!(session.status(), SenderStatus::Pairing);
    assert!(!session.trusted_devices().is_paired(receiver.device_id()));
    assert!(!session.take_keyframe_request());
    assert!(matches!(
        session.ingest_access_unit(b"must-not-send", true, 1, INITIAL_STREAM_EPOCH),
        Err(SenderError::MediaNotReady)
    ));
    assert_eq!(session.pending_packets(), 0);

    let active_session = session.session.expect("active session");
    session.inject_control_payload_for_session_for_test(
        SessionId(active_session.0 + 1),
        ControlPayload::PairingApproval(approval.clone()),
    );
    assert!(!session.trusted_devices().is_paired(receiver.device_id()));

    session
        .inject_control_payload_for_test(ControlPayload::PairingApproval(approval))
        .expect("inject approval");
    assert_eq!(session.status(), SenderStatus::Pairing);
    assert!(session.trusted_devices().is_paired(receiver.device_id()));
    assert!(!session.take_keyframe_request());

    let complete = PairingComplete {
        transcript_hash: transcript_hash.to_vec(),
        identity_signature: picoo_pairing::sign_transcript_phase(
            &receiver,
            &transcript_hash,
            PAIRING_COMPLETE_PHASE,
        )
        .to_vec(),
    };
    session
        .inject_control_payload_for_test(ControlPayload::PairingComplete(complete))
        .expect("inject completion");

    assert_eq!(session.status(), SenderStatus::Streaming);
    assert!(
        session.take_keyframe_request(),
        "receiver completion must request IDR before first encode"
    );

    let loaded = TrustedDeviceStore::load_from_path(&store_path).expect("load");
    assert!(loaded.is_paired(receiver.device_id()));
}

#[test]
fn failed_trusted_device_persist_rolls_back_memory_state() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut session = SenderSession::new(MemoryTransport::new());
    session.trusted.upsert(picoo_pairing::TrustedDevice {
        device_id: "receiver-rollback".into(),
        device_name: "Receiver".into(),
        public_key: vec![1, 2, 3],
        certificate_fingerprint: "rollback".into(),
        paired_at_ms: 1,
        last_connected_at_ms: None,
    });
    // Writing JSON to a directory is guaranteed to fail. The in-memory
    // trust decision must remain unchanged when persistence does not commit.
    session.trusted_store_path = Some(dir.path().to_path_buf());

    assert!(session.remove_trusted_device("receiver-rollback").is_err());
    assert!(session.trusted.is_paired("receiver-rollback"));
}

#[test]
fn unknown_receiver_cannot_disable_pairing() {
    let mut session = SenderSession::new(MemoryTransport::new());
    session
        .connect(Endpoint {
            host: "127.0.0.1".into(),
            port: 4433,
        })
        .expect("connect");
    session.send_client_hello().expect("hello");

    let receiver = picoo_pairing::DeviceIdentity::generate("Unknown").expect("identity");
    let (hello, _) = signed_server_hello(&session, &receiver, false);
    session
        .inject_control_payload_for_test(ControlPayload::ServerHello(hello))
        .expect("inject bypass attempt");

    assert_eq!(session.status(), SenderStatus::Disconnected);
    assert_eq!(
        session.last_session_error(),
        Some("UNTRUSTED_PAIRING_BYPASS")
    );
    assert!(!session.is_connected());
}

#[test]
fn sender_rejects_receiver_without_matching_private_key_proof() {
    let mut session = SenderSession::new(MemoryTransport::new());
    session
        .connect(Endpoint {
            host: "127.0.0.1".into(),
            port: 4433,
        })
        .expect("connect");
    session.send_client_hello().expect("hello");

    let receiver = picoo_pairing::DeviceIdentity::generate("Receiver").expect("identity");
    session
        .trusted_devices_mut()
        .upsert(picoo_pairing::trusted_device_from_pairing(
            receiver.device_id(),
            receiver.device_name(),
            receiver.public_key(),
            1,
        ));
    let (mut hello, _) = signed_server_hello(&session, &receiver, false);
    hello.identity_signature = vec![0; 64];
    session
        .inject_control_payload_for_test(ControlPayload::ServerHello(hello))
        .expect("inject invalid proof");

    assert_eq!(session.status(), SenderStatus::Disconnected);
    assert_eq!(session.last_session_error(), Some("INVALID_RECEIVER_PROOF"));
    assert!(!session.is_connected());
    assert!(session.pairing_short_code().is_none());
}

#[test]
fn old_server_proof_cannot_be_replayed_on_a_new_connection() {
    let mut session = SenderSession::new(MemoryTransport::new());
    let endpoint = Endpoint {
        host: "127.0.0.1".into(),
        port: 4433,
    };
    session.connect(endpoint.clone()).expect("first connect");
    session.send_client_hello().expect("first hello");
    let receiver = picoo_pairing::DeviceIdentity::generate("Receiver").expect("identity");
    let (old_hello, _) = signed_server_hello(&session, &receiver, true);

    session.disconnect();
    session.connect(endpoint).expect("second connect");
    session.send_client_hello().expect("second hello");
    session
        .inject_control_payload_for_test(ControlPayload::ServerHello(old_hello))
        .expect("inject replayed proof");

    assert_eq!(session.status(), SenderStatus::Disconnected);
    assert_eq!(session.last_session_error(), Some("INVALID_RECEIVER_PROOF"));
    assert!(!session.is_connected());
}

#[test]
fn privileged_control_is_rejected_until_receiver_is_authenticated() {
    use picoo_protocol::control::{camera_command, CameraCommand};

    let mut session = SenderSession::new(MemoryTransport::new());
    session
        .connect(Endpoint {
            host: "127.0.0.1".into(),
            port: 4433,
        })
        .expect("connect");
    session.send_client_hello().expect("hello");
    let receiver = picoo_pairing::DeviceIdentity::generate("Receiver").expect("identity");
    let (hello, _) = signed_server_hello(&session, &receiver, true);
    session
        .inject_control_payload_for_test(ControlPayload::ServerHello(hello))
        .expect("server hello");

    session
        .inject_control_payload_for_test(ControlPayload::CameraCommand(CameraCommand {
            command: camera_command::Command::SwitchCamera as i32,
            resolution: None,
            mirrored: false,
        }))
        .expect("inject unauthorized command");

    assert_eq!(session.status(), SenderStatus::Disconnected);
    assert_eq!(
        session.last_session_error(),
        Some("CONTROL_PAYLOAD_NOT_ALLOWED")
    );
    assert!(session.take_camera_command().is_none());
}
