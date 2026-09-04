use picoo_pairing::TrustedDeviceStore;
use picoo_session::SenderStatus;

use super::*;

fn create_test_sender() -> *mut std::ffi::c_void {
    let identity = picoo_pairing::DeviceIdentity::generate("Test Sender").expect("identity");
    let identity_handle = Box::into_raw(Box::new(identity)) as *mut std::ffi::c_void;
    let sender = picoo_sender_create(identity_handle);
    picoo_identity_destroy(identity_handle);
    sender
}

#[test]
fn protocol_name_cstr() {
    let ptr = picoo_protocol_name();
    assert!(!ptr.is_null());
}

#[test]
fn sender_rejects_offline_ingest_via_ffi() {
    assert!(picoo_sender_create(std::ptr::null_mut()).is_null());
    let handle = create_test_sender();
    assert!(!handle.is_null());
    let data = b"test-nalu";
    let mut out = 0u32;
    assert_eq!(
        picoo_sender_ingest_access_unit(
            handle,
            data.as_ptr(),
            data.len(),
            1,
            42,
            42,
            1,
            0,
            1,
            720,
            &mut out,
        ),
        -2
    );
    assert_eq!(out, 0);
    let mut stats = [0.0f64; 8];
    assert_eq!(
        picoo_sender_last_receiver_stats(handle, stats.as_mut_ptr(), stats.len()),
        1,
        "no ReceiverStats yet"
    );
    let mut w = 0u32;
    let mut h = 0u32;
    let mut mirrored = 0i32;
    assert_eq!(
        picoo_sender_take_camera_command(handle, &mut w, &mut h, &mut mirrored),
        0,
        "no CameraCommand pending"
    );
    assert_eq!(picoo_sender_disconnect(handle), 0);
    let mut snapshot = PicooSenderSnapshot::default();
    assert_eq!(picoo_sender_snapshot(handle, &mut snapshot), 0);
    assert_eq!(snapshot.status, SenderStatus::Disconnected.as_code());
    assert_eq!(snapshot.stream_epoch, picoo_sender::INITIAL_STREAM_EPOCH);
    picoo_sender_destroy(handle);
}

#[test]
fn sender_clones_the_supplied_signing_identity() {
    let identity = picoo_pairing::DeviceIdentity::generate("Durable Sender").expect("identity");
    let expected_id = identity.device_id().to_owned();
    let identity_handle = Box::into_raw(Box::new(identity)) as *mut std::ffi::c_void;
    let sender_handle = picoo_sender_create(identity_handle);
    assert!(!sender_handle.is_null());

    picoo_identity_destroy(identity_handle);
    let sender = unsafe { &*(sender_handle as *mut crate::handles::SenderInner) };
    assert_eq!(
        sender
            .session
            .lock()
            .expect("sender lock")
            .identity()
            .device_id(),
        expected_id
    );
    picoo_sender_destroy(sender_handle);
}

#[test]
fn sender_remove_trusted_device_c_abi_validates_and_persists_removal() {
    use std::ffi::CString;

    let missing = CString::new("missing-receiver").unwrap();
    assert_eq!(
        picoo_sender_remove_trusted_device(std::ptr::null_mut(), missing.as_ptr()),
        -1
    );

    let dir = tempfile::tempdir().expect("tempdir");
    let store_path = dir.path().join("trusted.json");
    let mut seeded_store = TrustedDeviceStore::new();
    let paired_identity = picoo_pairing::DeviceIdentity::from_secret_bytes("Studio Mac", &[7; 32])
        .expect("paired identity");
    let paired_device_id = paired_identity.device_id().to_owned();
    seeded_store.upsert(picoo_pairing::trusted_device_from_pairing(
        paired_identity.device_id(),
        paired_identity.device_name(),
        paired_identity.public_key(),
        100,
    ));
    seeded_store
        .save_to_path(&store_path)
        .expect("seed trusted store");
    let store = CString::new(store_path.to_str().unwrap()).unwrap();
    let handle = create_test_sender();
    assert!(!handle.is_null());
    assert_eq!(picoo_sender_attach_trusted_store(handle, store.as_ptr()), 0);
    assert_eq!(
        picoo_sender_remove_trusted_device(handle, std::ptr::null()),
        -1
    );
    assert_eq!(
        picoo_sender_remove_trusted_device(handle, missing.as_ptr()),
        0
    );
    let paired = CString::new(paired_device_id.as_str()).unwrap();
    assert_eq!(
        picoo_sender_remove_trusted_device(handle, paired.as_ptr()),
        1
    );
    let persisted = TrustedDeviceStore::load_from_path(&store_path).expect("reload store");
    assert!(!persisted.is_paired(&paired_device_id));
    picoo_sender_destroy(handle);
}

#[test]
fn identity_load_roundtrip_via_ffi() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("id.json");
    let path_c = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
    let name_c = std::ffi::CString::new("TestPhone").unwrap();
    let handle = picoo_identity_load_or_create(path_c.as_ptr(), name_c.as_ptr());
    assert!(!handle.is_null());
    let mut id_buf = [0u8; 64];
    let n = picoo_identity_device_id(
        handle,
        id_buf.as_mut_ptr() as *mut std::ffi::c_char,
        id_buf.len(),
    );
    assert!(n > 0);
    let mut key = [0u8; 32];
    assert_eq!(
        picoo_identity_public_key(handle, key.as_mut_ptr(), key.len()),
        32
    );
    picoo_identity_destroy(handle);

    let again = picoo_identity_load_or_create(path_c.as_ptr(), name_c.as_ptr());
    assert!(!again.is_null());
    let mut key2 = [0u8; 32];
    assert_eq!(
        picoo_identity_public_key(again, key2.as_mut_ptr(), key2.len()),
        32
    );
    assert_eq!(key, key2);
    picoo_identity_destroy(again);
}

#[test]
fn extract_sps_pps_via_ffi() {
    let sps = [0x67u8, 0x42, 0x00, 0x0a];
    let pps = [0x68u8, 0xce, 0x3c, 0x80];
    let mut annex = Vec::new();
    annex.extend_from_slice(&[0, 0, 0, 1]);
    annex.extend_from_slice(&sps);
    annex.extend_from_slice(&[0, 0, 0, 1]);
    annex.extend_from_slice(&pps);
    let mut sps_out = [0u8; 64];
    let mut pps_out = [0u8; 64];
    let mut sps_len = sps_out.len();
    let mut pps_len = pps_out.len();
    assert_eq!(
        picoo_h264_extract_sps_pps(
            annex.as_ptr(),
            annex.len(),
            sps_out.as_mut_ptr(),
            &mut sps_len,
            pps_out.as_mut_ptr(),
            &mut pps_len,
        ),
        0
    );
    assert_eq!(&sps_out[..sps_len], &sps);
    assert_eq!(&pps_out[..pps_len], &pps);
}

#[test]
fn sender_snapshot_is_coherent_before_capabilities() {
    let handle = create_test_sender();
    assert!(!handle.is_null());
    let mut snapshot = PicooSenderSnapshot::default();
    assert_eq!(picoo_sender_snapshot(handle, &mut snapshot), 0);
    assert_eq!(snapshot.receiver_max_height, 0);
    assert_eq!(snapshot.active_height, 1080);
    assert!(snapshot.current_bitrate_bps > 0);
    picoo_sender_destroy(handle);
}

#[test]
fn encoder_started_fact_requires_the_matching_transaction() {
    let handle = create_test_sender();
    assert!(!handle.is_null());
    let pending = picoo_sender_begin_stream_reconfiguration(handle, 720);
    assert!(pending > picoo_sender::INITIAL_STREAM_EPOCH);
    let mut directive = PicooEncoderDirective::default();
    assert_eq!(
        picoo_sender_peek_encoder_directive(handle, &mut directive),
        0
    );
    let transaction = picoo_sender_encoder_transaction_id(handle, pending);
    assert!(transaction > 0);
    assert_eq!(
        picoo_sender_report_encoder_started(handle, transaction, 7, pending + 1, 720),
        0
    );
    assert_eq!(
        picoo_sender_report_encoder_started(handle, transaction, 7, pending, 720),
        1
    );
    let mut snapshot = PicooSenderSnapshot::default();
    assert_eq!(picoo_sender_snapshot(handle, &mut snapshot), 0);
    assert_eq!(snapshot.stream_epoch, picoo_sender::INITIAL_STREAM_EPOCH);
    assert_eq!(
        picoo_sender_report_encoder_failed(handle, transaction, 0),
        1
    );
    assert_eq!(picoo_sender_encoder_transaction_id(handle, pending), 0);
    picoo_sender_destroy(handle);
}

#[test]
fn platform_sender_status_codes_match_rust_abi() {
    let kotlin = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../apps/android/app/src/main/kotlin/com/picoo/camera/jni/SenderStatusCodes.kt"
    ));
    let swift = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../apps/ios/PicooCamera/SenderModels.swift"
    ));
    let cases = [
        ("DISCONNECTED", "disconnected", SenderStatus::Disconnected),
        ("DISCOVERING", "discovering", SenderStatus::Discovering),
        ("PAIRING", "pairing", SenderStatus::Pairing),
        ("CONNECTING", "connecting", SenderStatus::Connecting),
        ("NEGOTIATING", "negotiating", SenderStatus::Negotiating),
        ("STREAMING", "streaming", SenderStatus::Streaming),
        ("RECONNECTING", "reconnecting", SenderStatus::Reconnecting),
        (
            "PERMISSION_REQUIRED",
            "permissionRequired",
            SenderStatus::PermissionRequired,
        ),
        (
            "NETWORK_UNSTABLE",
            "networkUnstable",
            SenderStatus::NetworkUnstable,
        ),
    ];
    for (kotlin_name, swift_name, status) in cases {
        let code = status.as_code();
        assert!(
            kotlin.contains(&format!("const val {kotlin_name} = {code}")),
            "Kotlin status {kotlin_name} drifted from Rust ABI"
        );
        assert!(
            swift.contains(&format!("case {swift_name} = {code}")),
            "Swift status {swift_name} drifted from Rust ABI"
        );
    }
}

#[test]
fn export_diagnostics_with_session_includes_redacted_host() {
    use std::ffi::CString;
    use std::fs;

    let dir = tempfile::tempdir().expect("tempdir");
    let store_path = dir.path().join("trusted.json");
    let out_path = dir.path().join("diag.json");
    fs::write(
        &store_path,
        r#"{"format":"picoo-camera-ed25519-trust","devices":[],"pending_identity_replacement":null,"next_identity_replacement_revision":1}"#,
    )
    .expect("empty store");

    let store = CString::new(store_path.to_str().unwrap()).unwrap();
    let platform = CString::new("android").unwrap();
    let version = CString::new("0.1.0").unwrap();
    let role = CString::new("sender").unwrap();
    let status = CString::new("Streaming").unwrap();
    let host = CString::new("192.168.1.42:4433").unwrap();
    let out = CString::new(out_path.to_str().unwrap()).unwrap();

    assert_eq!(
        picoo_export_diagnostics_to_path_with_session(
            store.as_ptr(),
            platform.as_ptr(),
            version.as_ptr(),
            role.as_ptr(),
            status.as_ptr(),
            12,
            34,
            0,
            host.as_ptr(),
            out.as_ptr(),
        ),
        0
    );
    let json = fs::read_to_string(&out_path).expect("read");
    // pretty-printed: `"includes_video": false`
    assert!(
        json.contains("\"includes_video\": false"),
        "PRIVACY-002 no-video flag missing: {json}"
    );
    assert!(json.contains("\"role\": \"sender\""), "{json}");
    assert!(json.contains("\"status\": \"Streaming\""), "{json}");
    assert!(json.contains("xxx"), "peer host must be redacted: {json}");
    assert!(!json.contains("192.168.1.42"), "{json}");
    assert_eq!(json.matches("\"access_units\": 12").count(), 1, "{json}");
    assert_eq!(json.matches("\"packets\": 34").count(), 1, "{json}");
    assert!(
        !json.contains("ingress_"),
        "session counters must be role-neutral: {json}"
    );
}
