use std::time::{Duration, Instant};

use picoo_pairing::{TrustedDevice, TrustedDeviceStore};
use picoo_sender::SenderSession;
use picoo_session::ReceiverStatus;
use picoo_transport::{Endpoint, QuicSenderTransport};

use crate::{
    run_loopback_access_unit, run_paired_loopback_access_unit, ReceiverSession,
    PAIRING_CHALLENGE_TTL,
};

fn use_stub_decoder(receiver: &mut ReceiverSession) {
    receiver.set_decoder_for_test(Box::new(picoo_media_decode::StubDecoder::new()));
}

#[test]
fn decoder_is_flushed_at_every_session_teardown_boundary() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct FlushCounter(Arc<AtomicUsize>);

    impl picoo_media_decode::AccessUnitDecoder for FlushCounter {
        fn decode_access_unit(
            &mut self,
            _access_unit: &[u8],
            _stream_config: Option<&picoo_protocol::control::StreamConfig>,
        ) -> Result<Option<picoo_media_decode::DecodedFrame>, picoo_media_decode::DecodeError>
        {
            Ok(None)
        }

        fn flush(
            &mut self,
        ) -> Result<Option<picoo_media_decode::DecodedFrame>, picoo_media_decode::DecodeError>
        {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(None)
        }
    }

    let flushes = Arc::new(AtomicUsize::new(0));
    let mut receiver = ReceiverSession::new();
    receiver.set_decoder_for_test(Box::new(FlushCounter(Arc::clone(&flushes))));

    receiver
        .inject_peer_disconnect_for_test()
        .expect("peer disconnect flush");
    assert_eq!(flushes.load(Ordering::SeqCst), 1);

    receiver.set_permit_unpaired_video(true);
    receiver
        .handle_stop_stream(picoo_transport::SessionId(1))
        .expect("StopStream flush");
    assert_eq!(flushes.load(Ordering::SeqCst), 2);

    receiver.close();
    assert_eq!(flushes.load(Ordering::SeqCst), 3);
}

fn pump_pair_for(
    receiver: &mut ReceiverSession,
    sender: &mut SenderSession<QuicSenderTransport>,
    duration: Duration,
) {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        receiver.pump().expect("rx pump");
        sender.pump().expect("tx pump");
        std::thread::sleep(Duration::from_millis(2));
    }
}

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
    assert_eq!(receiver.status(), ReceiverStatus::VirtualCameraUnavailable);
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
    use_stub_decoder(&mut receiver);
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
    receiver.confirm_pairing_locally();
    let _ = sender.send_pairing_confirm(&identity.receiver_id);
    for _ in 0..40 {
        receiver.pump().ok();
        sender.pump().ok();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_ne!(receiver.status(), ReceiverStatus::Streaming);
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
fn placeholder_mode_bars_and_logo_publish_distinct_frames() {
    // AC-D-SET-01: Logo / Black / Bars must produce distinct waiting frames.
    use picoo_frame_hub::PlaceholderMode;

    let mut receiver = ReceiverSession::new();
    receiver.set_placeholder_mode(PlaceholderMode::Logo);
    receiver.publish_waiting_placeholder().expect("logo");
    let logo = receiver
        .latest_frame()
        .expect("logo frame")
        .pixel_data
        .clone();

    receiver.set_placeholder_mode(PlaceholderMode::Black);
    receiver.publish_waiting_placeholder().expect("black");
    let black = receiver
        .latest_frame()
        .expect("black frame")
        .pixel_data
        .clone();

    receiver.set_placeholder_mode(PlaceholderMode::Bars);
    receiver.publish_waiting_placeholder().expect("bars");
    let bars = receiver
        .latest_frame()
        .expect("bars frame")
        .pixel_data
        .clone();

    assert_ne!(logo.as_ref(), black.as_ref(), "Logo ≠ Black");
    assert_ne!(bars.as_ref(), black.as_ref(), "Bars ≠ Black");
    assert_ne!(bars.as_ref(), logo.as_ref(), "Bars ≠ Logo");
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
    for _ in 0..100 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        if receiver.stats().packets_dropped_unpaired > 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    assert_eq!(receiver.stats().access_units, 0);
    assert!(receiver.stats().packets_dropped_unpaired > 0);
    assert_eq!(receiver.status(), ReceiverStatus::Connecting);
}

#[test]
fn unpaired_start_stream_is_rejected() {
    // REQ-PICOO-PAIRING-003: StartStream before pairing completes → SessionError UNPAIRED.
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
        .send_client_hello("unpaired-phone", "Unpaired", &[4, 4, 4])
        .expect("hello");

    for _ in 0..100 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        if receiver.pairing_short_code().is_some() && sender.pairing_short_code().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(receiver.pairing_short_code().is_some());
    assert_eq!(receiver.status(), ReceiverStatus::Pairing);

    let rejected_before = receiver.stats().control_rejected_unpaired;
    sender.send_start_stream().expect("start stream");

    for _ in 0..100 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        if sender.last_session_error() == Some("UNPAIRED") {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    assert_eq!(sender.last_session_error(), Some("UNPAIRED"));
    assert!(receiver.stats().control_rejected_unpaired > rejected_before);
    assert_ne!(receiver.status(), ReceiverStatus::Streaming);
}

#[test]
fn paired_start_stop_stream_and_camera_command_roundtrip() {
    // Control-plane: paired StartStream, CameraCommand (SWITCH_FRONT), StopStream.
    use picoo_protocol::control::{camera_command, CameraCommand};

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
        .send_client_hello("ctrl-phone", "Ctrl", &[5, 5, 5])
        .expect("hello");
    for _ in 0..100 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        if receiver.pairing_short_code().is_some() && sender.pairing_short_code().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    receiver.confirm_pairing_locally();
    sender
        .send_pairing_confirm(&identity.receiver_id)
        .expect("confirm");
    for _ in 0..100 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        if receiver.status() == ReceiverStatus::Streaming {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(receiver.status(), ReceiverStatus::Streaming);

    // Explicit StartStream while already paired/streaming is idempotent.
    sender.send_start_stream().expect("start");
    for _ in 0..40 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(receiver.status(), ReceiverStatus::Streaming);
    assert_eq!(receiver.stats().control_rejected_unpaired, 0);

    let cmd = CameraCommand {
        command: camera_command::Command::SwitchFront as i32,
        resolution: None,
        mirrored: false,
    };
    receiver.send_camera_command(cmd).expect("camera cmd");
    for _ in 0..40 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        if sender.take_camera_command().is_some() {
            // Re-fetch: take already consumed — assert via flag below.
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    // send again to assert payload (previous take may have raced)
    receiver
        .send_camera_command(CameraCommand {
            command: camera_command::Command::SwitchBack as i32,
            resolution: None,
            mirrored: false,
        })
        .expect("camera cmd 2");
    let mut got = None;
    for _ in 0..40 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        if let Some(c) = sender.take_camera_command() {
            got = Some(c);
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let got = got.expect("CameraCommand delivered to sender");
    assert_eq!(got.command, camera_command::Command::SwitchBack as i32);

    // PUC-005 / ABR: SetResolution 480p (854×480) must round-trip like 720/1080.
    receiver
        .send_camera_command(CameraCommand {
            command: camera_command::Command::SetResolution as i32,
            resolution: Some(picoo_protocol::control::Resolution {
                width: 854,
                height: 480,
            }),
            mirrored: false,
        })
        .expect("camera cmd 480p");
    let mut got_res = None;
    for _ in 0..40 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        if let Some(c) = sender.take_camera_command() {
            got_res = Some(c);
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let got_res = got_res.expect("SetResolution 480 delivered");
    assert_eq!(
        got_res.command,
        camera_command::Command::SetResolution as i32
    );
    let res = got_res.resolution.expect("resolution payload");
    assert_eq!((res.width, res.height), (854, 480));

    sender.send_stop_stream().expect("stop");
    for _ in 0..100 {
        receiver.pump().expect("receiver pump");
        sender.pump().ok();
        if matches!(
            receiver.status(),
            ReceiverStatus::Discovering
                | ReceiverStatus::Disconnected
                | ReceiverStatus::Reconnecting
        ) {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        matches!(
            receiver.status(),
            ReceiverStatus::Discovering
                | ReceiverStatus::Disconnected
                | ReceiverStatus::Reconnecting
        ),
        "after StopStream status={:?}",
        receiver.status()
    );
}

#[test]
fn unpaired_stop_stream_is_ignored_without_teardown() {
    // StopStream while still Pairing must not tear down into Streaming teardown paths
    // and must not clear the pending short code (idempotent / no-op for unpaired).
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
        .send_client_hello("stop-unpaired", "StopU", &[6, 6, 6])
        .expect("hello");
    for _ in 0..100 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        if receiver.pairing_short_code().is_some() && sender.pairing_short_code().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(receiver.status(), ReceiverStatus::Pairing);
    let code = receiver
        .pairing_short_code()
        .expect("short code")
        .to_string();
    let rejected_before = receiver.stats().control_rejected_unpaired;

    sender.send_stop_stream().expect("stop");
    for _ in 0..80 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        std::thread::sleep(Duration::from_millis(2));
    }

    assert_eq!(receiver.status(), ReceiverStatus::Pairing);
    assert_eq!(receiver.pairing_short_code(), Some(code.as_str()));
    assert!(receiver.stats().control_rejected_unpaired > rejected_before);
}

#[test]
fn camera_command_rejected_while_unpaired() {
    // REQ-PICOO-PAIRING-003: CameraCommand requires paired video path.
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
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if sender.is_connected() && receiver.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    sender
        .send_client_hello("cam-rej", "CamRej", &[1, 1, 1])
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

    use picoo_protocol::control::{camera_command, CameraCommand};
    let err = receiver
        .send_camera_command(CameraCommand {
            command: camera_command::Command::SwitchFront as i32,
            resolution: None,
            mirrored: false,
        })
        .expect_err("must reject");
    assert!(
        err.to_string().contains("paired") || err.to_string().contains("CameraCommand"),
        "unexpected err: {err}"
    );
    assert!(receiver.stats().control_rejected_unpaired >= 1);
}

#[test]
fn unpaired_video_keeps_shared_ring_on_placeholder() {
    // REQ-PICOO-PAIRING-003 / VCAM-003: unpaired datagrams must not drive VCam ring.
    use picoo_frame_hub::{
        SharedFrameRingConsumer, SharedFrameRingProducer, DEFAULT_MAX_FRAME_BYTES,
    };

    let ring_name = format!(
        "picoo-unpaired-ring-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let flink = SharedFrameRingProducer::flink_path(&ring_name);
    let _ = std::fs::remove_file(&flink);

    let mut receiver = ReceiverSession::new();
    receiver
        .attach_shared_ring(&ring_name)
        .expect("attach shared ring");
    let consumer =
        SharedFrameRingConsumer::open(&ring_name, DEFAULT_MAX_FRAME_BYTES).expect("consumer");
    let before = consumer.latest_frame().expect("placeholder");
    assert_eq!(before.timestamp_us, 0);
    assert!(before.width >= 640);
    let before_seq = before.sequence;
    let before_y0 = before.nv12.first().copied().unwrap_or(0);

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

    for frame_id in 1..=20u64 {
        sender
            .ingest_and_flush(format!("unpaired-{frame_id}").as_bytes(), true, frame_id, 1)
            .expect("ingest");
        for _ in 0..8 {
            receiver.pump().expect("rx");
            sender.pump().ok();
            std::thread::sleep(Duration::from_micros(100));
        }
    }

    assert_eq!(receiver.stats().access_units, 0);
    assert!(receiver.stats().packets_dropped_unpaired > 0);
    let after = consumer.latest_frame().expect("still placeholder");
    assert_eq!(after.timestamp_us, 0, "ring must stay on placeholder ts=0");
    assert_eq!(after.width, before.width);
    assert_eq!(after.height, before.height);
    // Sequence may bump if placeholder republished; pixels must not become a live frame.
    assert!(
        after.sequence >= before_seq,
        "seq regresses: before={before_seq} after={}",
        after.sequence
    );
    // Branded placeholder has non-zero Y near brand; a solid live stub would differ — ensure
    // we did not publish a tiny decoded frame.
    assert!(after.nv12.len() >= before.nv12.len().saturating_sub(0));
    assert_eq!(after.nv12.len(), before.nv12.len());
    let _ = before_y0;
    let _ = std::fs::remove_file(&flink);
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
    receiver.confirm_pairing_locally();

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
fn pairing_confirm_before_desktop_confirm_is_ignored() {
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

    receiver.confirm_pairing_locally();
    sender
        .send_pairing_confirm(&identity.receiver_id)
        .expect("confirm after desktop ack");
    let streaming_deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < streaming_deadline {
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
        receiver.latest_frame().is_some_and(|f| f.timestamp_us > 0),
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
        sender.set_stream_config(StreamConfigParams::default());
    }

    for _ in 0..100 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        if receiver.stream_config().is_some() && sender.receiver_capabilities().is_some() {
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
fn stream_epoch_bump_requests_keyframe() {
    // PUC-005 / REQ-PICOO-MEDIA-003 — Receiver half: epoch↑ → IDR request.
    use picoo_sender::StreamConfigParams;
    use picoo_session::ReceiverStatus;

    let mut receiver = ReceiverSession::new();
    receiver.set_jitter_target_ms(0);
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

    let mut cfg = StreamConfigParams {
        stream_epoch: 1,
        ..Default::default()
    };
    sender.set_stream_config(cfg.clone());
    let mut got_first_idr = false;
    for _ in 0..50 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if sender.take_keyframe_request() {
            got_first_idr = true;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        got_first_idr,
        "first StreamConfig must request IDR (SESSION-004 / MEDIA-003)"
    );

    cfg.stream_epoch = sender.begin_stream_reconfiguration();
    assert_eq!(cfg.stream_epoch, 2);
    assert!(sender.take_keyframe_request());
    sender.set_stream_config(cfg.clone());
    assert!(sender.report_encoder_height(cfg.height, cfg.stream_epoch));
    let mut got_idr = false;
    for _ in 0..80 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if sender.take_keyframe_request() {
            got_idr = true;
        }
        if got_idr
            && receiver
                .stream_config()
                .is_some_and(|c| c.stream_epoch == 2)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(got_idr, "epoch bump must request IDR");
    assert_eq!(receiver.stream_config().map(|c| c.stream_epoch), Some(2));

    // A candidate epoch is not accepted until native output confirms it. This
    // prevents QUIC datagrams from racing ahead of the reliable StreamConfig.
    let access_units_before = receiver.stats().access_units;
    let future_epoch = sender.begin_stream_reconfiguration();
    assert_eq!(future_epoch, 3);
    assert!(sender.take_keyframe_request());
    assert!(sender
        .ingest_and_flush(b"future-epoch", true, 1, future_epoch)
        .is_err());
    assert!(sender.cancel_stream_reconfiguration(future_epoch));
    assert_eq!(receiver.stats().access_units, access_units_before);
    assert_eq!(receiver.stream_config().map(|c| c.stream_epoch), Some(2));
}

#[test]
fn remote_mirrored_flips_framehub_nv12() {
    // REQ-PICOO-MEDIA-004 — remote StreamConfig.mirrored applied before FrameHub.
    use picoo_frame_hub::nv12_byte_size;
    use picoo_sender::StreamConfigParams;
    use picoo_session::ReceiverStatus;

    let width = 4u32;
    let height = 2u32;
    let mut pattern = vec![128u8; nv12_byte_size(width, height)];
    pattern[0] = 10;
    pattern[1] = 20;
    pattern[2] = 30;
    pattern[3] = 40;

    let mut receiver = ReceiverSession::new();
    use_stub_decoder(&mut receiver);
    receiver.set_jitter_target_ms(0);
    receiver.trusted_devices_mut().upsert(TrustedDevice {
        device_id: "android-sender".into(),
        device_name: "Pixel".into(),
        public_key: vec![9, 9, 9],
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
        receiver.pump().ok();
        sender.pump().ok();
        if sender.is_connected() && receiver.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    sender
        .send_client_hello("android-sender", "Pixel", &[9, 9, 9])
        .expect("hello");
    for _ in 0..100 {
        receiver.pump().ok();
        sender.pump().ok();
        if receiver.status() == ReceiverStatus::Streaming {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    let cfg = StreamConfigParams {
        width,
        height,
        mirrored: true,
        stream_epoch: 1,
        ..Default::default()
    };
    sender.set_stream_config(cfg);
    for _ in 0..100 {
        receiver.pump().ok();
        sender.pump().ok();
        if receiver
            .stream_config()
            .is_some_and(|config| config.width == width && config.height == height)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(receiver
        .stream_config()
        .is_some_and(|config| config.width == width && config.height == height));

    sender
        .ingest_access_unit(&pattern, true, 1, 1)
        .expect("ingest");
    sender.flush_pending().expect("flush");
    for _ in 0..100 {
        receiver.pump().ok();
        sender.pump().ok();
        if receiver
            .latest_frame()
            .is_some_and(|frame| frame.width == width && frame.height == height)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    let frame = receiver.latest_frame().expect("frame in hub");
    assert_eq!(frame.width, width);
    assert_eq!(frame.height, height);
    let y = &frame.pixel_data.as_ref()[..4];
    assert_eq!(
        y,
        &[40, 30, 20, 10],
        "Y plane must be horizontally mirrored"
    );
}

#[test]
fn stream_config_rotation_overrides_decoder_rotation() {
    // REQ-PICOO-MEDIA-009 / PUC-005: FrameHub publishes Sender StreamConfig.rotation.
    use picoo_frame_hub::nv12_byte_size;
    use picoo_sender::StreamConfigParams;

    let width = 4u32;
    let height = 2u32;
    let pattern = vec![42u8; nv12_byte_size(width, height)];

    let mut receiver = ReceiverSession::new();
    use_stub_decoder(&mut receiver);
    receiver.set_jitter_target_ms(0);
    receiver.trusted_devices_mut().upsert(TrustedDevice {
        device_id: "rot-phone".into(),
        device_name: "Rot".into(),
        public_key: vec![7, 7, 7],
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
        receiver.pump().ok();
        sender.pump().ok();
        if sender.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    sender
        .send_client_hello("rot-phone", "Rot", &[7, 7, 7])
        .expect("hello");
    for _ in 0..100 {
        receiver.pump().ok();
        sender.pump().ok();
        if receiver.status() == ReceiverStatus::Streaming {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    let cfg = StreamConfigParams {
        width,
        height,
        rotation: 90,
        ..Default::default()
    };
    sender.set_stream_config(cfg);
    for _ in 0..100 {
        receiver.pump().ok();
        sender.pump().ok();
        if receiver.stream_config().is_some_and(|config| {
            config.width == width && config.height == height && config.rotation == 90
        }) {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(receiver.stream_config().is_some_and(|config| {
        config.width == width && config.height == height && config.rotation == 90
    }));

    sender
        .ingest_access_unit(&pattern, true, 1, 1)
        .expect("ingest");
    sender.flush_pending().expect("flush");
    for _ in 0..100 {
        receiver.pump().ok();
        sender.pump().ok();
        if receiver
            .latest_frame()
            .is_some_and(|frame| frame.width == height && frame.height == width)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    let frame = receiver.latest_frame().expect("frame");
    // Pixels are upright; metadata cleared after apply (REQ-PICOO-MEDIA-009).
    assert_eq!(frame.rotation, 0);
    assert_eq!(frame.width, height); // 90° swaps dims
    assert_eq!(frame.height, width);
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
fn disconnect_holds_last_frame_then_shows_placeholder() {
    use crate::ReceiverIdentity;

    let identity = ReceiverIdentity::default();
    let mut receiver = ReceiverSession::new().with_identity(identity.clone());
    use_stub_decoder(&mut receiver);
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
    for _ in 0..200 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if receiver.pairing_short_code().is_some() && sender.pairing_short_code().is_some() {
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
    let live_ts = receiver.latest_frame().expect("live frame").timestamp_us;
    assert!(live_ts > 0);

    receiver
        .inject_peer_disconnect_for_test()
        .expect("disconnect reset");
    assert_eq!(receiver.status(), ReceiverStatus::Reconnecting);
    assert_eq!(
        receiver.latest_frame().expect("held frame").timestamp_us,
        live_ts
    );

    std::thread::sleep(Duration::from_millis(80));
    receiver.pump().expect("finalize hold");
    assert_eq!(receiver.status(), ReceiverStatus::Discovering);
    let placeholder = receiver.latest_frame().expect("placeholder");
    assert_eq!(placeholder.timestamp_us, 0);
    // FRAME-005: reconnect copy (not idle waiting) after last-frame hold.
    let recon = picoo_frame_hub::reconnecting_placeholder();
    assert_eq!(placeholder.pixel_data, recon);
}

#[test]
fn default_jitter_holds_au_until_target_delay() {
    // REQ-PICOO-SESSION-002: default 50ms target delays decode until media clock catches up.
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
fn run_paired_loopback_soak(soak_secs: u64, sample_every: u64) {
    let identity = crate::ReceiverIdentity::default();
    let mut receiver = ReceiverSession::new().with_identity(identity.clone());
    #[cfg(target_vendor = "apple")]
    use_stub_decoder(&mut receiver);
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
        if receiver.pairing_short_code().is_some() && sender.pairing_short_code().is_some() {
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

    // Prefer real H.264 on Linux so soak stresses OpenH264→FrameHub (REQ-PICOO-SESSION-005).
    #[cfg(all(not(windows), not(target_vendor = "apple")))]
    let soak_au: Vec<u8> = {
        use openh264::encoder::Encoder;
        use openh264::formats::YUVBuffer;
        use picoo_packet::extract_sps_pps;
        use picoo_sender::StreamConfigParams;

        let width = 160usize;
        let height = 120usize;
        let mut planes = vec![128u8; width * height * 3 / 2];
        for y in 0..height {
            for x in 0..width {
                planes[y * width + x] = ((x + y) % 200 + 20) as u8;
            }
        }
        let yuv = YUVBuffer::from_vec(planes, width, height);
        let mut encoder = Encoder::new().expect("openh264 encoder");
        let annex = encoder.encode(&yuv).expect("encode").to_vec();
        let (sps, pps) = extract_sps_pps(&annex).expect("SPS/PPS");
        sender.set_stream_config(StreamConfigParams {
            width: width as u32,
            height: height as u32,
            fps: 30,
            bitrate_bps: 500_000,
            stream_epoch: 1,
            mirrored: false,
            rotation: 0,
            sps,
            pps,
        });
        for _ in 0..50 {
            receiver.pump().expect("rx");
            sender.pump().expect("tx");
            if sender.stream_config_sent() {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        annex
    };
    #[cfg(any(windows, target_vendor = "apple"))]
    let soak_au: Vec<u8> = b"soak-frame-stub".to_vec();

    let deadline = std::time::Instant::now() + Duration::from_secs(soak_secs);
    let start = std::time::Instant::now();
    let mut next_sample = std::time::Instant::now();
    let mut samples: Vec<(u64, u64, u64)> = Vec::new(); // (elapsed_s, au, rss_kb)
    let mut frame_id = 1u64;

    while std::time::Instant::now() < deadline {
        sender
            .ingest_and_flush(&soak_au, frame_id % 30 == 1, frame_id, 1)
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
            next_sample += Duration::from_secs(sample_every.max(1));
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

/// CI-friendly SESSION-005 harness smoke (full 2h remains `--ignored` / SOAK_SECONDS=7200).
#[test]
fn soak_harness_smoke_five_seconds() {
    run_paired_loopback_soak(5, 2);
}

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
    run_paired_loopback_soak(soak_secs, sample_every);
}

#[test]
fn paired_loopback_remains_usable_under_five_percent_loss() {
    // PRD §21 / REQ-PICOO-SESSION-006: ~5% video datagram loss must not stall the session.
    use picoo_pairing::TrustedDevice;
    use picoo_session::ReceiverStatus;
    use picoo_testkit::LossyVideoTransport;
    use picoo_transport::{Endpoint, QuicSenderTransport};

    let loss_ratio: f64 = std::env::var("LOSS_RATIO")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.05);

    let mut receiver = ReceiverSession::new();
    use_stub_decoder(&mut receiver);
    receiver.set_jitter_target_ms(0);
    receiver.trusted_devices_mut().upsert(TrustedDevice {
        device_id: "lossy-phone".into(),
        device_name: "Lossy".into(),
        public_key: vec![7, 7, 7],
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

    let lossy = LossyVideoTransport::new(QuicSenderTransport::new(), loss_ratio);
    let mut sender = SenderSession::new(lossy);
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
        .send_client_hello("lossy-phone", "Lossy", &[7, 7, 7])
        .expect("hello");
    for _ in 0..100 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if receiver.status() == ReceiverStatus::Streaming {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(receiver.status(), ReceiverStatus::Streaming);

    let mut frames_seen = 0u64;
    let mut last_au = receiver.stats().access_units;
    let mut stalled_rounds = 0u32;
    for frame_id in 1..=400u64 {
        // Prefer keyframes so a drop does not permanently break the stub decode chain.
        let is_key = frame_id % 5 == 1;
        let payload = format!("lossy-au-{frame_id}");
        sender
            .ingest_and_flush(payload.as_bytes(), is_key, frame_id, 1)
            .expect("ingest");
        for _ in 0..12 {
            receiver.pump().expect("rx");
            sender.pump().ok();
        }
        if receiver.latest_frame().is_some() {
            frames_seen += 1;
        }
        let au = receiver.stats().access_units;
        if au == last_au {
            stalled_rounds += 1;
        } else {
            stalled_rounds = 0;
            last_au = au;
        }
        // Allow brief stalls under loss, but not a permanent hang.
        assert!(
            stalled_rounds < 80,
            "session stalled under {loss_ratio} loss after frame_id={frame_id} au={au}"
        );
        std::thread::sleep(Duration::from_millis(2));
    }

    let observed = sender.transport().observed_drop_ratio();
    assert!(
        frames_seen > 20,
        "expected many FrameHub updates under loss, got {frames_seen}"
    );
    assert!(
        receiver.stats().access_units > 30,
        "expected reassembled AUs under loss, got {}",
        receiver.stats().access_units
    );
    assert!(
        (0.02..0.12).contains(&observed),
        "expected ~{loss_ratio} observed drops, got {observed}"
    );

    // PRD §21: after recovery, delay must not accumulate past 1s.
    // Clear loss, push fresh keyframes, then require FrameHub frame_age < 1000ms.
    sender.transport_mut().set_drop_ratio(0.0);
    for frame_id in 401..=430u64 {
        let payload = format!("recover-au-{frame_id}");
        sender
            .ingest_and_flush(payload.as_bytes(), true, frame_id, 1)
            .expect("recover ingest");
        for _ in 0..8 {
            receiver.pump().expect("rx");
            sender.pump().ok();
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        receiver.latest_frame().is_some(),
        "expected FrameHub frame after lossless recovery window"
    );
    // Wait for the 1s ReceiverStats interval, while continuing to publish fresh frames
    // so frame_age reflects live recovery rather than idle stall.
    let t_stats = std::time::Instant::now();
    let mut recover_id = 431u64;
    while t_stats.elapsed() < Duration::from_millis(1100) {
        let payload = format!("recover-au-{recover_id}");
        sender
            .ingest_and_flush(payload.as_bytes(), true, recover_id, 1)
            .expect("recover keep-alive");
        recover_id += 1;
        for _ in 0..6 {
            receiver.pump().expect("rx");
            sender.pump().ok();
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    for _ in 0..20 {
        receiver.pump().ok();
        sender.pump().ok();
        std::thread::sleep(Duration::from_millis(10));
    }
    // Direct FrameHub age (decode timestamp → now) — PRD §21 recovery bound.
    let frame = receiver.latest_frame().expect("recovered frame");
    let now_us = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0);
    let hub_age_ms = now_us.saturating_sub(frame.timestamp_us) as f64 / 1000.0;
    assert!(
        hub_age_ms < 1_000.0,
        "FrameHub age piled up after recovery: {hub_age_ms}ms (PRD §21 <1s)"
    );
    if let Some(stats) = receiver.last_stats() {
        assert!(
            stats.frame_age_ms < 1_000.0,
            "stats frame_age piled up after recovery: {}ms (PRD §21 <1s)",
            stats.frame_age_ms
        );
    }
}

#[test]
fn paired_loopback_e2e_latency_p50_under_budget() {
    // PRD §21 / REQ-PICOO-SESSION-007: loopback ingest→FrameHub P50/P95 latency budget.
    // Full camera→VCam P95 needs devices; this closes the transport/decode path gate on Linux.
    use picoo_pairing::TrustedDevice;
    use picoo_session::ReceiverStatus;
    use picoo_transport::{Endpoint, QuicSenderTransport};
    use std::time::Instant;

    let mut receiver = ReceiverSession::new();
    use_stub_decoder(&mut receiver);
    receiver.set_jitter_target_ms(0);
    receiver.trusted_devices_mut().upsert(TrustedDevice {
        device_id: "lat-phone".into(),
        device_name: "Lat".into(),
        public_key: vec![3, 3, 3],
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
        receiver.pump().ok();
        sender.pump().ok();
        if sender.is_connected() && receiver.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    sender
        .send_client_hello("lat-phone", "Lat", &[3, 3, 3])
        .expect("hello");
    for _ in 0..100 {
        receiver.pump().ok();
        sender.pump().ok();
        if receiver.status() == ReceiverStatus::Streaming {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    let mut samples_ms = Vec::new();
    let mut last_seq = 0u64;
    for frame_id in 1..=80u64 {
        let payload = format!("lat-{frame_id}");
        let t0 = Instant::now();
        sender
            .ingest_and_flush(payload.as_bytes(), true, frame_id, 1)
            .expect("ingest");
        let mut observed = None;
        for _ in 0..200 {
            receiver.pump().ok();
            sender.pump().ok();
            if let Some(frame) = receiver.latest_frame() {
                if frame.sequence > last_seq {
                    last_seq = frame.sequence;
                    observed = Some(t0.elapsed().as_secs_f64() * 1000.0);
                    break;
                }
            }
            std::thread::sleep(Duration::from_micros(100));
        }
        if let Some(ms) = observed {
            samples_ms.push(ms);
        }
    }

    assert!(
        samples_ms.len() >= 40,
        "need enough latency samples, got {}",
        samples_ms.len()
    );
    samples_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p50 = samples_ms[samples_ms.len() / 2];
    let p95 = samples_ms[(samples_ms.len() as f64 * 0.95) as usize];
    eprintln!(
        "loopback ingest→FrameHub latency_ms p50={p50:.2} p95={p95:.2} n={}",
        samples_ms.len()
    );
    // Healthy LAN budgets from PRD (transport path only on loopback should be far below).
    assert!(p50 < 150.0, "P50 {p50}ms exceeds 150ms budget");
    assert!(p95 < 250.0, "P95 {p95}ms exceeds 250ms budget");
}

#[cfg(all(not(windows), not(target_vendor = "apple")))]
#[test]
fn paired_openh264_remains_usable_under_five_percent_loss() {
    // SESSION-006 with real H.264 → FrameHub (not stub AUs).
    use picoo_pairing::TrustedDevice;
    use picoo_sender::StreamConfigParams;
    use picoo_session::ReceiverStatus;
    use picoo_testkit::LossyVideoTransport;
    use picoo_transport::{Endpoint, QuicSenderTransport};

    let loss_ratio: f64 = std::env::var("LOSS_RATIO")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.05);
    let (au, sps, pps) = openh264_au(160, 120, 17);

    let mut receiver = ReceiverSession::new();
    receiver.set_jitter_target_ms(0);
    receiver.trusted_devices_mut().upsert(TrustedDevice {
        device_id: "lossy-h264".into(),
        device_name: "LossyH264".into(),
        public_key: vec![7, 7, 7],
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
    let lossy = LossyVideoTransport::new(QuicSenderTransport::new(), loss_ratio);
    let mut sender = SenderSession::new(lossy);
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
        .send_client_hello("lossy-h264", "LossyH264", &[7, 7, 7])
        .expect("hello");
    for _ in 0..100 {
        receiver.pump().ok();
        sender.pump().ok();
        if receiver.status() == ReceiverStatus::Streaming {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(receiver.status(), ReceiverStatus::Streaming);
    sender.set_stream_config(StreamConfigParams {
        width: 160,
        height: 120,
        fps: 30,
        bitrate_bps: 400_000,
        stream_epoch: 1,
        mirrored: false,
        rotation: 0,
        sps,
        pps,
    });
    for _ in 0..50 {
        receiver.pump().ok();
        sender.pump().ok();
        if sender.stream_config_sent() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    let mut frames_seen = 0u64;
    let mut last_au = receiver.stats().access_units;
    let mut stalled = 0u32;
    for frame_id in 1..=120u64 {
        let is_key = frame_id % 5 == 1;
        sender
            .ingest_and_flush(&au, is_key, frame_id, 1)
            .expect("ingest");
        for _ in 0..16 {
            receiver.pump().ok();
            sender.pump().ok();
            std::thread::sleep(Duration::from_micros(100));
        }
        if receiver.latest_frame().is_some_and(|f| f.timestamp_us > 0) {
            frames_seen += 1;
        }
        let au_n = receiver.stats().access_units;
        if au_n == last_au {
            stalled += 1;
        } else {
            stalled = 0;
            last_au = au_n;
        }
        assert!(
            stalled < 60,
            "openh264 path stalled under {loss_ratio} loss at frame={frame_id}"
        );
    }
    assert!(
        frames_seen >= 20,
        "need usable decoded frames under loss, got {frames_seen}"
    );
}

#[cfg(all(not(windows), not(target_vendor = "apple")))]
#[test]
fn paired_openh264_e2e_latency_p50_under_budget() {
    // SESSION-007: OpenH264 ingest→FrameHub P50/P95.
    use picoo_pairing::TrustedDevice;
    use picoo_sender::StreamConfigParams;
    use picoo_session::ReceiverStatus;
    use picoo_transport::{Endpoint, QuicSenderTransport};
    use std::time::Instant;

    let (au, sps, pps) = openh264_au(160, 120, 21);
    let mut receiver = ReceiverSession::new();
    receiver.set_jitter_target_ms(0);
    receiver.trusted_devices_mut().upsert(TrustedDevice {
        device_id: "lat-h264".into(),
        device_name: "LatH264".into(),
        public_key: vec![3, 3, 3],
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
        receiver.pump().ok();
        sender.pump().ok();
        if sender.is_connected() && receiver.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    sender
        .send_client_hello("lat-h264", "LatH264", &[3, 3, 3])
        .expect("hello");
    for _ in 0..100 {
        receiver.pump().ok();
        sender.pump().ok();
        if receiver.status() == ReceiverStatus::Streaming {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    sender.set_stream_config(StreamConfigParams {
        width: 160,
        height: 120,
        fps: 30,
        bitrate_bps: 400_000,
        stream_epoch: 1,
        mirrored: false,
        rotation: 0,
        sps,
        pps,
    });
    for _ in 0..50 {
        receiver.pump().ok();
        sender.pump().ok();
        if sender.stream_config_sent() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    let mut samples_ms = Vec::new();
    let mut last_seq = 0u64;
    for frame_id in 1..=40u64 {
        let t0 = Instant::now();
        sender
            .ingest_and_flush(&au, true, frame_id, 1)
            .expect("ingest");
        let mut observed = None;
        for _ in 0..300 {
            receiver.pump().ok();
            sender.pump().ok();
            if let Some(frame) = receiver.latest_frame() {
                if frame.sequence > last_seq && frame.timestamp_us > 0 {
                    last_seq = frame.sequence;
                    observed = Some(t0.elapsed().as_secs_f64() * 1000.0);
                    break;
                }
            }
            std::thread::sleep(Duration::from_micros(100));
        }
        if let Some(ms) = observed {
            samples_ms.push(ms);
        }
    }
    assert!(
        samples_ms.len() >= 20,
        "need enough openh264 latency samples, got {}",
        samples_ms.len()
    );
    samples_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p50 = samples_ms[samples_ms.len() / 2];
    let p95 = samples_ms[(samples_ms.len() as f64 * 0.95) as usize];
    eprintln!(
        "openh264 ingest→FrameHub latency_ms p50={p50:.2} p95={p95:.2} n={}",
        samples_ms.len()
    );
    assert!(p50 < 150.0, "openh264 P50 {p50}ms exceeds 150ms");
    assert!(p95 < 250.0, "openh264 P95 {p95}ms exceeds 250ms");
}

#[test]
fn paired_connect_to_streaming_under_three_seconds() {
    // PUC-002 / REQ-PICOO-DISCOVERY-006: paired connect establish < 3s (QUIC hello→Streaming).
    use std::time::Instant;

    let mut samples_ms = Vec::new();
    for round in 0..5u32 {
        let mut receiver = ReceiverSession::new();
        receiver.trusted_devices_mut().upsert(TrustedDevice {
            device_id: format!("conn-phone-{round}"),
            device_name: "Conn".into(),
            public_key: vec![4, 4, 4],
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
        let t0 = Instant::now();
        sender
            .connect(Endpoint {
                host: bind.ip().to_string(),
                port: bind.port(),
            })
            .expect("connect");
        for _ in 0..400 {
            receiver.pump().ok();
            sender.pump().ok();
            if sender.is_connected() {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        sender
            .send_client_hello(&format!("conn-phone-{round}"), "Conn", &[4, 4, 4])
            .expect("hello");
        for _ in 0..400 {
            receiver.pump().ok();
            sender.pump().ok();
            if receiver.status() == ReceiverStatus::Streaming {
                samples_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(
            receiver.status(),
            ReceiverStatus::Streaming,
            "round {round} never reached Streaming"
        );
    }
    samples_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p50 = samples_ms[samples_ms.len() / 2];
    eprintln!("paired connect→Streaming latency_ms samples={samples_ms:?} p50={p50:.2}");
    assert!(
        p50 < 3_000.0,
        "paired connect P50 {p50}ms exceeds 3s budget"
    );
    for ms in &samples_ms {
        assert!(*ms < 3_000.0, "sample {ms}ms exceeds 3s budget");
    }
}

#[test]
fn brief_disconnect_recovers_streaming_under_five_seconds() {
    // PRD §8.1 / PUC-006: brief disconnect recovery < 5s on healthy loopback.
    use picoo_transport::CloseReason;
    use std::time::Instant;

    let mut receiver = ReceiverSession::new();
    receiver.set_jitter_target_ms(0);
    receiver.trusted_devices_mut().upsert(TrustedDevice {
        device_id: "recov-phone".into(),
        device_name: "Recov".into(),
        public_key: vec![5, 5, 5],
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
    for _ in 0..400 {
        receiver.pump().ok();
        sender.pump().ok();
        if sender.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    sender
        .send_client_hello("recov-phone", "Recov", &[5, 5, 5])
        .expect("hello");
    for _ in 0..400 {
        receiver.pump().ok();
        sender.pump().ok();
        if receiver.status() == ReceiverStatus::Streaming {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(receiver.status(), ReceiverStatus::Streaming);

    sender.disconnect_for_test(CloseReason::Timeout);
    let t0 = Instant::now();
    let mut recovered = false;
    for _ in 0..400 {
        receiver.pump().ok();
        sender.pump().ok();
        if receiver.status() == ReceiverStatus::Streaming && sender.is_connected() {
            // Wait until sender also reports Streaming (ServerHello after reconnect).
            if sender.status() == picoo_session::SenderStatus::Streaming
                || sender.status() == picoo_session::SenderStatus::Negotiating
            {
                // Prefer full Streaming when possible.
            }
            if sender.status() == picoo_session::SenderStatus::Streaming {
                recovered = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
    eprintln!("brief disconnect recovery_ms={elapsed_ms:.2} recovered={recovered}");
    assert!(recovered, "did not recover Streaming after disconnect");
    assert!(
        elapsed_ms < 5_000.0,
        "recovery {elapsed_ms}ms exceeds 5s budget"
    );
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

#[cfg(all(not(windows), not(target_vendor = "apple")))]
#[test]
fn paired_openh264_access_unit_reaches_frame_hub() {
    // REQ-PICOO-MEDIA-005/006: real Annex-B H.264 through QUIC → decode → FrameHub.
    use openh264::encoder::Encoder;
    use openh264::formats::YUVBuffer;
    use picoo_frame_hub::nv12_byte_size;
    use picoo_packet::extract_sps_pps;
    use picoo_pairing::TrustedDevice;
    use picoo_sender::StreamConfigParams;
    use picoo_session::ReceiverStatus;
    use picoo_transport::{Endpoint, QuicSenderTransport};

    let width = 160usize;
    let height = 120usize;
    let mut planes = vec![128u8; width * height * 3 / 2];
    for y in 0..height {
        for x in 0..width {
            planes[y * width + x] = ((x * 3 + y * 5) % 256) as u8;
        }
    }
    let yuv = YUVBuffer::from_vec(planes, width, height);
    let mut encoder = Encoder::new().expect("openh264 encoder");
    let bitstream = encoder.encode(&yuv).expect("encode");
    let annex = bitstream.to_vec();
    assert!(annex.len() > 64, "AU too small for OpenH264 path");
    let (sps, pps) = extract_sps_pps(&annex).expect("SPS/PPS from Annex-B");

    let mut receiver = ReceiverSession::new();
    receiver.set_jitter_target_ms(0);
    receiver.trusted_devices_mut().upsert(TrustedDevice {
        device_id: "h264-phone".into(),
        device_name: "H264".into(),
        public_key: vec![9, 9, 9],
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
    for _ in 0..500 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if sender.is_connected() && receiver.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    sender
        .send_client_hello("h264-phone", "H264", &[9, 9, 9])
        .expect("hello");
    for _ in 0..200 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if receiver.status() == ReceiverStatus::Streaming {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(receiver.status(), ReceiverStatus::Streaming);

    sender.set_stream_config(StreamConfigParams {
        width: width as u32,
        height: height as u32,
        fps: 30,
        bitrate_bps: 500_000,
        stream_epoch: 1,
        mirrored: false,
        rotation: 0,
        sps,
        pps,
    });
    for _ in 0..50 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if sender.stream_config_sent() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    sender
        .ingest_and_flush(&annex, true, 1, 1)
        .expect("ingest h264");
    for _ in 0..300 {
        receiver.pump().expect("rx");
        sender.pump().ok();
        if let Some(frame) = receiver.latest_frame() {
            if frame.width == width as u32 && frame.height == height as u32 {
                assert_eq!(
                    frame.pixel_data.len(),
                    nv12_byte_size(frame.width, frame.height)
                );
                assert!(
                    frame.pixel_data.iter().any(|b| *b != 16 && *b != 128),
                    "expected non-placeholder NV12 from OpenH264"
                );
                assert_eq!(receiver.stats().decode_invocations, 1);
                assert_eq!(receiver.stats().access_units, 1);
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!(
        "OpenH264 AU did not reach FrameHub at {}x{}; stats={:?}",
        width,
        height,
        receiver.stats()
    );
}

#[cfg(not(windows))]
#[test]
fn paired_avcc_length_prefixed_au_reaches_frame_hub() {
    // REQ-PICOO-PROTOCOL-005 / MEDIA-005: MediaCodec-shaped AVCC AU reaches the
    // platform decoder. The encoded fixture avoids a test-only native codec.
    use picoo_frame_hub::nv12_byte_size;
    use picoo_packet::{
        annex_b_to_length_prefixed, extract_sps_pps, is_length_prefixed_access_unit,
    };
    use picoo_pairing::TrustedDevice;
    use picoo_sender::StreamConfigParams;
    use picoo_session::ReceiverStatus;
    use picoo_testkit::H264_64X64_RED_IDR;
    use picoo_transport::{Endpoint, QuicSenderTransport};

    let width = 64usize;
    let height = 64usize;
    let (sps, pps) = extract_sps_pps(H264_64X64_RED_IDR).expect("sps/pps");
    let avcc = annex_b_to_length_prefixed(H264_64X64_RED_IDR).expect("avcc wrap");
    assert!(is_length_prefixed_access_unit(&avcc));

    let mut receiver = ReceiverSession::new();
    receiver.set_jitter_target_ms(0);
    receiver.trusted_devices_mut().upsert(TrustedDevice {
        device_id: "avcc-phone".into(),
        device_name: "Avcc".into(),
        public_key: vec![8, 8, 8],
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
    for _ in 0..500 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if sender.is_connected() && receiver.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    sender
        .send_client_hello("avcc-phone", "Avcc", &[8, 8, 8])
        .expect("hello");
    for _ in 0..200 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if receiver.status() == ReceiverStatus::Streaming {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(receiver.status(), ReceiverStatus::Streaming);
    sender.set_stream_config(StreamConfigParams {
        width: width as u32,
        height: height as u32,
        fps: 30,
        bitrate_bps: 500_000,
        stream_epoch: 1,
        mirrored: false,
        rotation: 0,
        sps,
        pps,
    });
    for _ in 0..50 {
        receiver.pump().ok();
        sender.pump().ok();
        if sender.stream_config_sent() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    sender
        .ingest_and_flush(&avcc, true, 1, 1)
        .expect("ingest avcc");
    for _ in 0..300 {
        receiver.pump().ok();
        sender.pump().ok();
        if let Some(frame) = receiver.latest_frame() {
            if frame.width == width as u32 && frame.height == height as u32 {
                assert_eq!(
                    frame.pixel_data.len(),
                    nv12_byte_size(frame.width, frame.height)
                );
                assert!(frame.pixel_data.iter().any(|b| *b != 16 && *b != 128));
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!(
        "AVCC AU did not reach FrameHub; stats={:?}",
        receiver.stats()
    );
}

#[cfg(target_os = "macos")]
#[test]
fn macos_videotoolbox_abr_epoch_resolution_recovery() {
    // REQ-PICOO-MEDIA-003/010/012: ABR epoch changes flow through QUIC and
    // rebuild VideoToolbox with the dimensions advertised by StreamConfig.
    use picoo_frame_hub::nv12_byte_size;
    use picoo_packet::extract_sps_pps;
    use picoo_protocol::control::ReceiverStats as ReceiverStatsMsg;
    use picoo_sender::StreamConfigParams;
    use picoo_testkit::{H264_1280X720_RED_IDR, H264_854X480_RED_IDR};
    use prost::Message;

    let mut receiver = ReceiverSession::new();
    receiver.set_jitter_target_ms(0);
    receiver.trusted_devices_mut().upsert(TrustedDevice {
        device_id: "macos-abr-phone".into(),
        device_name: "macOS ABR".into(),
        public_key: vec![4, 8, 0],
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
    for _ in 0..500 {
        receiver.pump().expect("receiver connect");
        sender.pump().expect("sender connect");
        if sender.is_connected() && receiver.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    sender
        .send_client_hello("macos-abr-phone", "macOS ABR", &[4, 8, 0])
        .expect("hello");
    for _ in 0..200 {
        receiver.pump().expect("receiver hello");
        sender.pump().expect("sender hello");
        if receiver.status() == ReceiverStatus::Streaming {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(receiver.status(), ReceiverStatus::Streaming);

    let inject_congestion = |sender: &mut SenderSession<QuicSenderTransport>| {
        for _ in 0..40 {
            let stats = ReceiverStatsMsg {
                packet_loss: 0.05,
                frame_age_ms: 250.0,
                ..Default::default()
            };
            let mut bytes = Vec::new();
            stats.encode(&mut bytes).expect("encode stats");
            sender
                .inject_control_for_test(bytes::Bytes::from(bytes))
                .expect("inject stats");
            if let Some(directive) = sender.pending_encoder_directive() {
                assert!(
                    sender.acknowledge_encoder_directive(directive.id, directive.target_height,)
                );
                return;
            }
        }
        panic!("ABR did not request a resolution downshift");
    };

    inject_congestion(&mut sender);
    assert_eq!(sender.bitrate_active_height(), 720);
    let (sps_720, pps_720) = extract_sps_pps(H264_1280X720_RED_IDR).expect("720p parameter sets");
    sender.set_stream_config(StreamConfigParams {
        width: 1280,
        height: 720,
        fps: 30,
        bitrate_bps: 3_000_000,
        stream_epoch: 2,
        sps: sps_720,
        pps: pps_720,
        ..Default::default()
    });
    for _ in 0..80 {
        receiver.pump().expect("receiver 720p config");
        sender.pump().expect("sender 720p config");
        if receiver
            .stream_config()
            .is_some_and(|config| config.stream_epoch == 2)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    sender
        .ingest_and_flush(H264_1280X720_RED_IDR, true, 2, 2)
        .expect("send 720p IDR");
    for _ in 0..300 {
        receiver.pump().expect("receiver 720p frame");
        sender.pump().ok();
        if receiver
            .latest_frame()
            .is_some_and(|frame| (frame.width, frame.height) == (1280, 720))
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let frame_720 = receiver.latest_frame().expect("720p frame");
    assert_eq!((frame_720.width, frame_720.height), (1280, 720));
    assert_eq!(frame_720.pixel_data.len(), nv12_byte_size(1280, 720));
    let sequence_720 = frame_720.sequence;

    inject_congestion(&mut sender);
    assert_eq!(sender.bitrate_active_height(), 480);
    let (sps_480, pps_480) = extract_sps_pps(H264_854X480_RED_IDR).expect("480p parameter sets");
    sender.set_stream_config(StreamConfigParams {
        width: 854,
        height: 480,
        fps: 30,
        bitrate_bps: 1_800_000,
        stream_epoch: 3,
        sps: sps_480,
        pps: pps_480,
        ..Default::default()
    });
    for _ in 0..80 {
        receiver.pump().expect("receiver 480p config");
        sender.pump().expect("sender 480p config");
        if receiver
            .stream_config()
            .is_some_and(|config| config.stream_epoch == 3)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    sender
        .ingest_and_flush(H264_854X480_RED_IDR, true, 3, 3)
        .expect("send 480p IDR");
    for _ in 0..300 {
        receiver.pump().expect("receiver 480p frame");
        sender.pump().ok();
        if receiver.latest_frame().is_some_and(|frame| {
            frame.sequence > sequence_720 && (frame.width, frame.height) == (854, 480)
        }) {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let frame_480 = receiver.latest_frame().expect("480p frame");
    assert!(frame_480.sequence > sequence_720);
    assert_eq!((frame_480.width, frame_480.height), (854, 480));
    assert_eq!(frame_480.pixel_data.len(), nv12_byte_size(854, 480));
}

#[cfg(not(windows))]
#[test]
fn thermal_hold_blocks_abr_upshift_on_sender() {
    // REQ-PICOO-MEDIA-010: host thermal force keeps ABR from requesting 1080p.
    use picoo_protocol::control::ReceiverStats as ReceiverStatsMsg;
    use picoo_transport::QuicSenderTransport;
    use prost::Message;

    let mut sender = SenderSession::new(QuicSenderTransport::new());
    sender.set_preferred_height(1080);
    assert!(sender.report_encoder_height(720, sender.current_stream_epoch()));
    sender.set_thermal_hold(true);
    assert_eq!(sender.bitrate_active_height(), 720);
    assert!(sender.thermal_hold());

    for _ in 0..80 {
        let stats = ReceiverStatsMsg {
            packet_loss: 0.0,
            frame_age_ms: 40.0,
            jitter_buffer_depth_ms: 40.0,
            ..Default::default()
        };
        let mut buf = Vec::new();
        stats.encode(&mut buf).expect("encode");
        sender
            .inject_control_for_test(bytes::Bytes::from(buf))
            .expect("inject");
        assert!(
            sender.pending_encoder_directive().is_none(),
            "thermal hold must suppress upshift hint"
        );
    }
    sender.set_thermal_hold(false);
    let mut up = false;
    for _ in 0..120 {
        let stats = ReceiverStatsMsg {
            packet_loss: 0.0,
            frame_age_ms: 40.0,
            jitter_buffer_depth_ms: 40.0,
            ..Default::default()
        };
        let mut buf = Vec::new();
        stats.encode(&mut buf).expect("encode");
        sender
            .inject_control_for_test(bytes::Bytes::from(buf))
            .expect("inject");
        if let Some(directive) = sender.pending_encoder_directive() {
            assert!(sender.acknowledge_encoder_directive(directive.id, directive.target_height,));
            up = true;
            break;
        }
    }
    assert!(up, "after thermal clear, ABR should request upshift");
    assert_eq!(sender.bitrate_active_height(), 1080);
}

#[cfg(all(not(windows), not(target_vendor = "apple")))]
#[test]
fn paired_openh264_publishes_to_shared_frame_ring() {
    // REQ-PICOO-FRAME-003 / VCAM-003: decode once → Shared Frame Ring for VCam consumer.
    use openh264::encoder::Encoder;
    use openh264::formats::YUVBuffer;
    use picoo_frame_hub::{
        nv12_byte_size, SharedFrameRingConsumer, SharedFrameRingProducer, DEFAULT_MAX_FRAME_BYTES,
    };
    use picoo_packet::extract_sps_pps;
    use picoo_pairing::TrustedDevice;
    use picoo_sender::StreamConfigParams;
    use picoo_session::ReceiverStatus;
    use picoo_transport::{Endpoint, QuicSenderTransport};

    let width = 160usize;
    let height = 120usize;
    let mut planes = vec![128u8; width * height * 3 / 2];
    for y in 0..height {
        for x in 0..width {
            planes[y * width + x] = ((x * 11 + y * 3) % 220 + 18) as u8;
        }
    }
    let yuv = YUVBuffer::from_vec(planes, width, height);
    let mut encoder = Encoder::new().expect("openh264 encoder");
    let annex = encoder.encode(&yuv).expect("encode").to_vec();
    let (sps, pps) = extract_sps_pps(&annex).expect("SPS/PPS");

    let ring_name = format!(
        "picoo-h264-ring-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let flink = SharedFrameRingProducer::flink_path(&ring_name);
    let _ = std::fs::remove_file(&flink);

    let mut receiver = ReceiverSession::new();
    receiver.set_jitter_target_ms(0);
    receiver
        .attach_shared_ring(&ring_name)
        .expect("attach shared ring");
    receiver.trusted_devices_mut().upsert(TrustedDevice {
        device_id: "ring-phone".into(),
        device_name: "Ring".into(),
        public_key: vec![4, 4, 4],
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

    let consumer =
        SharedFrameRingConsumer::open(&ring_name, DEFAULT_MAX_FRAME_BYTES).expect("consumer");
    // Placeholder is published on attach.
    let placeholder = consumer.latest_frame().expect("placeholder on ring");
    assert!(placeholder.sequence >= 1);
    let placeholder_seq = placeholder.sequence;

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
        .send_client_hello("ring-phone", "Ring", &[4, 4, 4])
        .expect("hello");
    for _ in 0..200 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if receiver.status() == ReceiverStatus::Streaming {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(receiver.status(), ReceiverStatus::Streaming);

    sender.set_stream_config(StreamConfigParams {
        width: width as u32,
        height: height as u32,
        fps: 30,
        bitrate_bps: 500_000,
        stream_epoch: 1,
        mirrored: false,
        rotation: 90,
        sps,
        pps,
    });
    for _ in 0..50 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if sender.stream_config_sent() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    sender.ingest_and_flush(&annex, true, 1, 1).expect("ingest");
    for _ in 0..300 {
        receiver.pump().expect("rx");
        sender.pump().ok();
        if let Some(view) = consumer.latest_frame() {
            if view.sequence > placeholder_seq
                && view.width == height as u32
                && view.height == width as u32
            {
                assert_eq!(view.nv12.len(), nv12_byte_size(view.width, view.height));
                assert_eq!(
                    view.rotation, 0,
                    "pixels upright after rotate; metadata cleared"
                );
                assert!(
                    view.nv12.iter().any(|b| *b != 16 && *b != 128),
                    "ring must carry decoded NV12, not placeholder grey"
                );
                let _ = std::fs::remove_file(&flink);
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let _ = std::fs::remove_file(&flink);
    panic!(
        "decoded H.264 did not appear on Shared Frame Ring; hub={:?} stats={:?}",
        receiver
            .latest_frame()
            .map(|f| (f.width, f.height, f.rotation)),
        receiver.stats()
    );
}

#[test]
fn paired_loopback_binds_lan_only_without_wan() {
    // REQ-PICOO-PRIVACY-005: discovery/transport stay on LAN; no WAN dependency.
    use picoo_pairing::TrustedDevice;
    use picoo_session::ReceiverStatus;
    use picoo_transport::{Endpoint, QuicSenderTransport};

    let mut receiver = ReceiverSession::new();
    use_stub_decoder(&mut receiver);
    receiver.set_jitter_target_ms(0);
    receiver.trusted_devices_mut().upsert(TrustedDevice {
        device_id: "lan-phone".into(),
        device_name: "LAN".into(),
        public_key: vec![1, 1, 1],
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
    assert!(
        bind.ip().is_loopback(),
        "receiver must bind loopback/LAN for PRIVACY-005, got {}",
        bind.ip()
    );

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
        .send_client_hello("lan-phone", "LAN", &[1, 1, 1])
        .expect("hello");
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
        .ingest_and_flush(b"lan-only-au", true, 1, 1)
        .expect("ingest");
    for _ in 0..100 {
        receiver.pump().expect("rx");
        sender.pump().ok();
        if receiver.latest_frame().is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("LAN loopback video path failed without WAN");
}

#[test]
fn mismatched_protocol_version_rejects_client_hello() {
    // ARCH-PICOO-PROTOCOL-001 / PROTOCOL-002: version negotiation fail-fast.
    use picoo_transport::{Endpoint, QuicSenderTransport};

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
        let _ = receiver.pump();
        sender.pump().ok();
        if sender.is_connected() && receiver.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    sender
        .send_client_hello_with_version("ver-phone", "Ver", &[1, 1, 1], "picoocam/9")
        .expect("send hello");
    let mut saw_protocol_err = false;
    for _ in 0..50 {
        if let Err(err) = receiver.pump() {
            let msg = err.to_string();
            if msg.contains("protocol_version") || msg.contains("unsupported") {
                saw_protocol_err = true;
                break;
            }
        }
        sender.pump().ok();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        saw_protocol_err
            || !matches!(
                receiver.status(),
                ReceiverStatus::Streaming | ReceiverStatus::Pairing | ReceiverStatus::Connecting
            ),
        "mismatched protocol_version must fail fast; status={:?}",
        receiver.status()
    );
    assert_ne!(receiver.status(), ReceiverStatus::Streaming);
}

#[test]
fn capabilities_720_only_are_applied_before_sender_stream_config() {
    // REQ-PICOO-MEDIA-002: platform applies the advertised limit, then commits epoch/config.
    use picoo_pairing::TrustedDevice;
    use picoo_sender::StreamConfigParams;
    use picoo_transport::{Endpoint, QuicSenderTransport};

    let mut receiver = ReceiverSession::new();
    receiver.set_jitter_target_ms(0);
    receiver.set_advertised_max_height(720);
    receiver.trusted_devices_mut().upsert(TrustedDevice {
        device_id: "cap-phone".into(),
        device_name: "Cap".into(),
        public_key: vec![2, 2, 2],
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
        receiver.pump().ok();
        sender.pump().ok();
        if sender.is_connected() && receiver.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    // Prefer 1080 before Caps arrive. Rust exposes the limit but does not claim
    // a native resolution change before the platform reports successful apply.
    sender.set_stream_config(StreamConfigParams {
        width: 1920,
        height: 1080,
        fps: 30,
        bitrate_bps: 6_000_000,
        stream_epoch: 1,
        mirrored: false,
        rotation: 0,
        sps: vec![0x67],
        pps: vec![0x68],
    });
    sender
        .send_client_hello("cap-phone", "Cap", &[2, 2, 2])
        .expect("hello");
    for _ in 0..200 {
        receiver.pump().ok();
        sender.pump().ok();
        if receiver.status() == ReceiverStatus::Streaming && sender.receiver_max_height() > 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(sender.receiver_max_height(), 720);
    assert_eq!(
        sender.pending_stream_config().map(|config| config.height),
        Some(1080)
    );

    let epoch = sender.begin_stream_reconfiguration();
    assert!(sender.report_encoder_height(720, epoch));
    sender.set_stream_config(StreamConfigParams {
        width: 1280,
        height: 720,
        fps: 30,
        bitrate_bps: 3_000_000,
        stream_epoch: 0,
        mirrored: false,
        rotation: 0,
        sps: vec![0x67],
        pps: vec![0x68],
    });
    for _ in 0..80 {
        sender.pump().ok();
        receiver.pump().ok();
        if receiver
            .stream_config()
            .is_some_and(|config| config.height == 720)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        receiver
            .stream_config()
            .map(|config| (config.height, config.stream_epoch)),
        Some((720, epoch))
    );
}

#[test]
fn manual_endpoint_connects_to_streaming() {
    // REQ-PICOO-DISCOVERY-007 / PUC-008: manual IP endpoint uses the normal QUIC path.
    use picoo_pairing::TrustedDevice;
    use picoo_transport::{Endpoint, QuicSenderTransport};

    let mut receiver = ReceiverSession::new();
    receiver.set_jitter_target_ms(0);
    receiver.trusted_devices_mut().upsert(TrustedDevice {
        device_id: "manual-phone".into(),
        device_name: "Manual".into(),
        public_key: vec![4, 4, 4],
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
        .expect("connect from manual endpoint");
    for _ in 0..200 {
        receiver.pump().ok();
        sender.pump().ok();
        if sender.is_connected() && receiver.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    sender
        .send_client_hello("manual-phone", "Manual", &[4, 4, 4])
        .expect("hello");
    for _ in 0..200 {
        receiver.pump().ok();
        sender.pump().ok();
        if receiver.status() == ReceiverStatus::Streaming {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(receiver.status(), ReceiverStatus::Streaming);
}

#[test]
fn reconnect_churn_smoke_five_rounds() {
    run_reconnect_churn(5);
}

#[test]
fn reconnect_churn_fifteen_rounds() {
    // Medium CI gate between smoke (5) and full PRD N=50 (`--ignored`).
    run_reconnect_churn(15);
}

#[test]
#[ignore = "PRD §20.6 N=50 reconnect churn; enable via --ignored"]
fn reconnect_churn_fifty_rounds() {
    run_reconnect_churn(50);
}

fn run_reconnect_churn(rounds: u32) {
    // REQ-PICOO-SESSION-004 / SESSION-008 / PRD §20.6.
    use picoo_pairing::TrustedDevice;
    use picoo_transport::{CloseReason, Endpoint, QuicSenderTransport};
    use std::time::Instant;

    let mut receiver = ReceiverSession::new();
    receiver.set_jitter_target_ms(0);
    receiver.set_last_frame_hold_for_test(Duration::from_millis(10));
    receiver.trusted_devices_mut().upsert(TrustedDevice {
        device_id: "churn-phone".into(),
        device_name: "Churn".into(),
        public_key: vec![5, 5, 5],
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
    for _ in 0..400 {
        receiver.pump().ok();
        sender.pump().ok();
        if sender.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    sender
        .send_client_hello("churn-phone", "Churn", &[5, 5, 5])
        .expect("hello");
    for _ in 0..400 {
        receiver.pump().ok();
        sender.pump().ok();
        if receiver.status() == ReceiverStatus::Streaming
            && sender.status() == picoo_session::SenderStatus::Streaming
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(receiver.status(), ReceiverStatus::Streaming);

    for round in 0..rounds {
        sender.disconnect_for_test(CloseReason::Timeout);
        let t0 = Instant::now();
        let mut recovered = false;
        for _ in 0..500 {
            receiver.pump().ok();
            sender.pump().ok();
            if receiver.status() == ReceiverStatus::Streaming
                && sender.status() == picoo_session::SenderStatus::Streaming
                && sender.is_connected()
            {
                recovered = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            recovered,
            "reconnect round {round}/{rounds} failed to recover Streaming"
        );
        assert!(
            t0.elapsed() < Duration::from_secs(5),
            "round {round} recovery {:?} exceeds 5s",
            t0.elapsed()
        );
    }
}

#[cfg(all(not(windows), not(target_vendor = "apple")))]
fn openh264_au(width: usize, height: usize, seed: u8) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    use openh264::encoder::Encoder;
    use openh264::formats::YUVBuffer;
    use picoo_packet::extract_sps_pps;

    let mut planes = vec![128u8; width * height * 3 / 2];
    for y in 0..height {
        for x in 0..width {
            planes[y * width + x] = ((x as u8).wrapping_mul(seed).wrapping_add(y as u8)) % 200 + 20;
        }
    }
    let yuv = YUVBuffer::from_vec(planes, width, height);
    let mut encoder = Encoder::new().expect("encoder");
    let annex = encoder.encode(&yuv).expect("encode").to_vec();
    let (sps, pps) = extract_sps_pps(&annex).expect("SPS/PPS");
    (annex, sps, pps)
}

#[cfg(all(not(windows), not(target_vendor = "apple")))]
#[test]
fn stream_epoch_bump_recovers_openh264_framehub_under_three_seconds() {
    // PUC-005 / REQ-PICOO-MEDIA-003: camera/epoch switch → new IDR in FrameHub <3s.
    use picoo_frame_hub::nv12_byte_size;
    use picoo_pairing::TrustedDevice;
    use picoo_sender::StreamConfigParams;
    use picoo_session::ReceiverStatus;
    use picoo_transport::{Endpoint, QuicSenderTransport};
    use std::time::Instant;

    let (au1, sps1, pps1) = openh264_au(854, 480, 3);
    let (au2, sps2, pps2) = openh264_au(854, 480, 9);

    let mut receiver = ReceiverSession::new();
    receiver.set_jitter_target_ms(0);
    receiver.trusted_devices_mut().upsert(TrustedDevice {
        device_id: "epoch-phone".into(),
        device_name: "Epoch".into(),
        public_key: vec![8, 8, 8],
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
    for _ in 0..500 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if sender.is_connected() && receiver.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    sender
        .send_client_hello("epoch-phone", "Epoch", &[8, 8, 8])
        .expect("hello");
    for _ in 0..200 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if receiver.status() == ReceiverStatus::Streaming {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(receiver.status(), ReceiverStatus::Streaming);

    sender.set_stream_config(StreamConfigParams {
        width: 854,
        height: 480,
        fps: 30,
        bitrate_bps: 500_000,
        stream_epoch: 1,
        mirrored: false,
        rotation: 0,
        sps: sps1,
        pps: pps1,
    });
    for _ in 0..40 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if sender.stream_config_sent() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    sender.ingest_and_flush(&au1, true, 1, 1).expect("au1");
    for _ in 0..200 {
        receiver.pump().expect("rx");
        sender.pump().ok();
        if receiver.latest_frame().is_some_and(|f| f.width == 854) {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(receiver.latest_frame().is_some());
    let before_au = receiver.stats().access_units;

    // Camera switch: epoch bump + new IDR.
    let t0 = Instant::now();
    let next_epoch = sender.begin_stream_reconfiguration();
    assert_eq!(next_epoch, 2);
    sender.set_stream_config(StreamConfigParams {
        width: 854,
        height: 480,
        fps: 30,
        bitrate_bps: 500_000,
        stream_epoch: next_epoch,
        mirrored: false,
        rotation: 0,
        sps: sps2,
        pps: pps2,
    });
    assert!(sender.report_encoder_height(480, next_epoch));
    for _ in 0..40 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if sender.stream_config_sent() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    // Sender should observe RequestKeyframe from epoch bump.
    let mut keyed = false;
    for _ in 0..30 {
        receiver.pump().ok();
        sender.pump().ok();
        if sender.take_keyframe_request() {
            keyed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(keyed, "epoch bump must request IDR");

    sender.ingest_and_flush(&au2, true, 2, 2).expect("au2");
    let mut recovered = false;
    for _ in 0..400 {
        receiver.pump().expect("rx");
        sender.pump().ok();
        if receiver.stats().access_units > before_au
            && receiver.latest_frame().is_some_and(|f| {
                f.width == 854
                    && f.pixel_data.len() == nv12_byte_size(854, 480)
                    && f.pixel_data.iter().any(|b| *b != 16 && *b != 128)
            })
        {
            recovered = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
    eprintln!("epoch_switch recovery_ms={elapsed_ms:.2} recovered={recovered}");
    assert!(recovered, "new-epoch frame missing after switch");
    assert!(
        elapsed_ms < 3_000.0,
        "epoch switch recovery {elapsed_ms}ms exceeds 3s PUC-005 budget"
    );
}

#[cfg(all(not(windows), not(target_vendor = "apple")))]
#[test]
fn midstream_resolution_change_openh264_updates_framehub() {
    // REQ-PICOO-MEDIA-002/010: mid-stream 480p → 720p with new SPS/PPS.
    use picoo_frame_hub::nv12_byte_size;
    use picoo_pairing::TrustedDevice;
    use picoo_sender::StreamConfigParams;
    use picoo_session::ReceiverStatus;
    use picoo_transport::{Endpoint, QuicSenderTransport};
    use std::time::Instant;

    let (au_lo, sps_lo, pps_lo) = openh264_au(854, 480, 5);
    let (au_hi, sps_hi, pps_hi) = openh264_au(1280, 720, 11);

    let mut receiver = ReceiverSession::new();
    receiver.set_jitter_target_ms(0);
    receiver.trusted_devices_mut().upsert(TrustedDevice {
        device_id: "res-phone".into(),
        device_name: "Res".into(),
        public_key: vec![6, 6, 6],
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
    for _ in 0..500 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if sender.is_connected() && receiver.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    sender
        .send_client_hello("res-phone", "Res", &[6, 6, 6])
        .expect("hello");
    for _ in 0..200 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if receiver.status() == ReceiverStatus::Streaming {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    sender.set_stream_config(StreamConfigParams {
        width: 854,
        height: 480,
        fps: 30,
        bitrate_bps: 400_000,
        stream_epoch: 1,
        mirrored: false,
        rotation: 0,
        sps: sps_lo,
        pps: pps_lo,
    });
    for _ in 0..40 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if sender.stream_config_sent() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    sender.ingest_and_flush(&au_lo, true, 1, 1).expect("lo");
    for _ in 0..200 {
        receiver.pump().expect("rx");
        sender.pump().ok();
        if receiver.latest_frame().is_some_and(|f| f.width == 854) {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(receiver.latest_frame().map(|f| f.width), Some(854));

    let t0 = Instant::now();
    let next_epoch = sender.begin_stream_reconfiguration();
    assert_eq!(next_epoch, 2);
    sender.set_stream_config(StreamConfigParams {
        width: 1280,
        height: 720,
        fps: 30,
        bitrate_bps: 1_200_000,
        stream_epoch: next_epoch,
        mirrored: false,
        rotation: 0,
        sps: sps_hi,
        pps: pps_hi,
    });
    assert!(sender.report_encoder_height(720, next_epoch));
    for _ in 0..40 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if sender.stream_config_sent() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    sender.ingest_and_flush(&au_hi, true, 2, 2).expect("hi");
    let mut ok = false;
    for _ in 0..400 {
        receiver.pump().expect("rx");
        sender.pump().ok();
        if let Some(frame) = receiver.latest_frame() {
            if frame.width == 1280 && frame.height == 720 {
                assert_eq!(frame.pixel_data.len(), nv12_byte_size(1280, 720));
                ok = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
    eprintln!("resolution_switch recovery_ms={elapsed_ms:.2} ok={ok}");
    assert!(ok, "FrameHub did not update to 1280x720");
    assert!(
        elapsed_ms < 3_000.0,
        "resolution switch {elapsed_ms}ms exceeds 3s budget"
    );
}

#[cfg(all(not(windows), not(target_vendor = "apple")))]
#[test]
fn incomplete_keyframe_requests_idr_and_recovers_framehub() {
    // REQ-PICOO-SESSION-003: incomplete IDR → RequestKeyframe → fresh IDR → FrameHub.
    use openh264::encoder::Encoder;
    use openh264::formats::YUVBuffer;
    use picoo_frame_hub::nv12_byte_size;
    use picoo_packet::extract_sps_pps;
    use picoo_pairing::TrustedDevice;
    use picoo_sender::StreamConfigParams;
    use picoo_session::ReceiverStatus;
    use picoo_testkit::DropKeyframeTailTransport;
    use picoo_transport::{Endpoint, QuicSenderTransport};

    let width = 160usize;
    let height = 120usize;
    let mut planes = vec![128u8; width * height * 3 / 2];
    for y in 0..height {
        for x in 0..width {
            planes[y * width + x] = ((x * 7 + y * 11) % 200 + 20) as u8;
        }
    }
    let yuv = YUVBuffer::from_vec(planes.clone(), width, height);
    let mut encoder = Encoder::new().expect("openh264 encoder");
    let annex = encoder.encode(&yuv).expect("encode").to_vec();
    let (sps, pps) = extract_sps_pps(&annex).expect("SPS/PPS");
    assert!(annex.len() > 32);

    // Pad with filler NAL so the AU spans ≥2 QUIC video fragments (~1124 B payload).
    let mut large_key = annex.clone();
    large_key.extend_from_slice(&[0, 0, 0, 1, 0x0c]);
    large_key.resize(large_key.len() + 1_300, 0x00);
    assert!(
        large_key.len() > 1_200,
        "padded AU must exceed one datagram payload"
    );

    let mut recovery_planes = planes;
    for y in 0..height {
        for x in 0..width {
            recovery_planes[y * width + x] = ((x * 13 + y * 17) % 180 + 30) as u8;
        }
    }
    let recovery_yuv = YUVBuffer::from_vec(recovery_planes, width, height);
    // The requested recovery frame must be a fresh IDR, not another P-frame
    // whose references may include the discarded incomplete access unit.
    let mut recovery_encoder = Encoder::new().expect("recovery OpenH264 encoder");
    let recovery_au = recovery_encoder
        .encode(&recovery_yuv)
        .expect("recovery encode")
        .to_vec();
    assert!(
        picoo_packet::split_annex_b_nals(&recovery_au)
            .iter()
            .any(|nal| nal.first().is_some_and(|byte| byte & 0x1f == 5)),
        "same-epoch recovery fixture must contain an IDR"
    );

    let mut receiver = ReceiverSession::new();
    receiver.set_jitter_target_ms(0);
    receiver.trusted_devices_mut().upsert(TrustedDevice {
        device_id: "idr-phone".into(),
        device_name: "Idr".into(),
        public_key: vec![9, 9, 9],
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

    let dropper = DropKeyframeTailTransport::new(QuicSenderTransport::new());
    let mut sender = SenderSession::new(dropper);
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
        .send_client_hello("idr-phone", "Idr", &[9, 9, 9])
        .expect("hello");
    for _ in 0..200 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if receiver.status() == ReceiverStatus::Streaming {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(receiver.status(), ReceiverStatus::Streaming);

    let stream_config = StreamConfigParams {
        width: width as u32,
        height: height as u32,
        fps: 30,
        bitrate_bps: 500_000,
        stream_epoch: 1,
        mirrored: false,
        rotation: 0,
        sps,
        pps,
    };
    sender.set_stream_config(stream_config.clone());
    for _ in 0..50 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if sender.stream_config_sent() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    // Baseline IDR (single-fragment) — tails not armed yet.
    sender
        .ingest_and_flush(&annex, true, 1, 1)
        .expect("baseline");
    for _ in 0..200 {
        receiver.pump().expect("rx");
        sender.pump().ok();
        if receiver.latest_frame().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(receiver.latest_frame().is_some(), "baseline frame missing");
    let before_seq = receiver.latest_frame().map(|f| f.sequence).unwrap_or(0);
    let _ = sender.take_keyframe_request();

    // Incomplete multi-fragment IDR: only fragment 0 arrives.
    sender.transport_mut().arm();
    sender
        .ingest_and_flush(&large_key, true, 2, 1)
        .expect("large incomplete");
    assert!(
        sender.transport_mut().dropped_tail_fragments >= 1,
        "expected keyframe tail drop"
    );

    // A newer AU cannot prove loss because QUIC Datagram may reorder across
    // frames. The bounded 120 ms reassembly deadline discards the partial IDR
    // and reports the loss exactly once.
    let mut keyed = false;
    for _ in 0..160 {
        receiver.pump().expect("rx reassembly deadline");
        sender.pump().expect("tx reassembly deadline");
        if sender.take_keyframe_request() {
            keyed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        keyed,
        "incomplete keyframe must produce EncoderCommand::RequestKeyframe"
    );
    assert_eq!(
        receiver.latest_frame().map(|frame| frame.sequence),
        Some(before_seq),
        "an incomplete IDR must not publish another FrameHub frame"
    );

    // A fresh IDR on the current epoch recovers FrameHub.
    let before_recovery_seq = receiver
        .latest_frame()
        .map(|frame| frame.sequence)
        .unwrap_or(0);
    sender.transport_mut().disarm();
    sender
        .ingest_and_flush(&recovery_au, true, 100, 1)
        .expect("recovery idr");
    let mut recovered = false;
    for _ in 0..400 {
        receiver.pump().expect("rx");
        sender.pump().ok();
        if let Some(frame) = receiver.latest_frame() {
            if frame.sequence > before_recovery_seq
                && frame.width == width as u32
                && frame.pixel_data.len() == nv12_byte_size(width as u32, height as u32)
                && frame.pixel_data.iter().any(|b| *b != 16 && *b != 128)
            {
                recovered = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        recovered,
        "FrameHub did not recover after RequestKeyframe IDR"
    );
}

#[cfg(all(not(windows), not(target_vendor = "apple")))]
#[test]
fn abr_downshift_updates_stream_config_and_framehub() {
    // REQ-PICOO-MEDIA-010: sustained congestion → DownshiftResolution → 720p StreamConfig → FrameHub.
    use openh264::encoder::Encoder;
    use openh264::formats::YUVBuffer;
    use picoo_frame_hub::nv12_byte_size;
    use picoo_packet::extract_sps_pps;
    use picoo_pairing::TrustedDevice;
    use picoo_protocol::control::ReceiverStats as ReceiverStatsMsg;
    use picoo_sender::StreamConfigParams;
    use picoo_session::ReceiverStatus;
    use picoo_transport::{Endpoint, QuicSenderTransport};
    use prost::Message;

    fn encode_pattern(w: usize, h: usize, seed: u8) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let mut planes = vec![128u8; w * h * 3 / 2];
        planes[..w * h].fill(seed.saturating_add(32));
        let yuv = YUVBuffer::from_vec(planes, w, h);
        let mut encoder = Encoder::new().expect("encoder");
        let annex = encoder.encode(&yuv).expect("encode").to_vec();
        let (sps, pps) = extract_sps_pps(&annex).expect("sps/pps");
        (annex, sps, pps)
    }

    let (au_hi, sps_hi, pps_hi) = encode_pattern(1920, 1080, 1);
    let (au_lo, sps_lo, pps_lo) = encode_pattern(1280, 720, 9);

    let mut receiver = ReceiverSession::new();
    receiver.set_jitter_target_ms(0);
    receiver.trusted_devices_mut().upsert(TrustedDevice {
        device_id: "abr-phone".into(),
        device_name: "Abr".into(),
        public_key: vec![5, 5, 5],
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
    for _ in 0..500 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if sender.is_connected() && receiver.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    sender
        .send_client_hello("abr-phone", "Abr", &[5, 5, 5])
        .expect("hello");
    for _ in 0..200 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if receiver.status() == ReceiverStatus::Streaming {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(receiver.status(), ReceiverStatus::Streaming);
    assert_eq!(sender.bitrate_active_height(), 1080);

    sender.set_stream_config(StreamConfigParams {
        width: 1920,
        height: 1080,
        fps: 30,
        bitrate_bps: 6_000_000,
        stream_epoch: 1,
        mirrored: false,
        rotation: 0,
        sps: sps_hi,
        pps: pps_hi,
    });
    for _ in 0..40 {
        receiver.pump().ok();
        sender.pump().ok();
        if sender.stream_config_sent() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    sender.ingest_and_flush(&au_hi, true, 1, 1).expect("hi");
    for _ in 0..200 {
        receiver.pump().ok();
        sender.pump().ok();
        if receiver.latest_frame().is_some_and(|f| f.width == 1920) {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(receiver.latest_frame().is_some_and(|f| f.width == 1920));

    // Sustained congestion → ABR downshift hint (same path Android MainActivity polls).
    let mut downshifted = false;
    for _ in 0..40 {
        let stats = ReceiverStatsMsg {
            packet_loss: 0.05,
            frame_age_ms: 250.0,
            ..Default::default()
        };
        let mut buf = Vec::new();
        stats.encode(&mut buf).expect("encode");
        sender
            .inject_control_for_test(bytes::Bytes::from(buf))
            .expect("inject");
        if let Some(directive) = sender.pending_encoder_directive() {
            assert!(sender.acknowledge_encoder_directive(directive.id, directive.target_height,));
            downshifted = true;
            break;
        }
    }
    assert!(downshifted, "ABR must request resolution downshift");
    assert_eq!(sender.bitrate_active_height(), 720);

    // Apply 720p StreamConfig + smaller AU (Android would call encoder.setResolution).
    // Must work while status is NetworkUnstable (congestion path) — REQ-PICOO-MEDIA-010.
    let cfg_lo = StreamConfigParams {
        width: 1280,
        height: 720,
        fps: 30,
        bitrate_bps: 3_000_000,
        stream_epoch: 2,
        mirrored: false,
        rotation: 0,
        sps: sps_lo,
        pps: pps_lo,
    };
    sender.set_stream_config(cfg_lo.clone());
    for _ in 0..80 {
        receiver.pump().ok();
        sender.pump().ok();
        if sender.stream_config_sent() && receiver.stream_config().is_some_and(|c| c.height == 720)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        receiver.stream_config().map(|c| (c.width, c.height)),
        Some((1280, 720)),
        "StreamConfig must be 1280x720 after ABR apply"
    );
    sender.ingest_and_flush(&au_lo, true, 2, 2).expect("lo");
    let mut ok = false;
    for _ in 0..400 {
        receiver.pump().ok();
        sender.pump().ok();
        if let Some(frame) = receiver.latest_frame() {
            if frame.width == 1280 && frame.height == 720 {
                assert_eq!(frame.pixel_data.len(), nv12_byte_size(1280, 720));
                ok = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(ok, "FrameHub must show post-downshift frames");

    // Second ABR rung: 720 → 480 (MEDIA-010 / PUC-006 weak-network floor).
    let mut downshifted_480 = false;
    for _ in 0..40 {
        let stats = ReceiverStatsMsg {
            packet_loss: 0.05,
            frame_age_ms: 250.0,
            ..Default::default()
        };
        let mut buf = Vec::new();
        stats.encode(&mut buf).expect("encode");
        sender
            .inject_control_for_test(bytes::Bytes::from(buf))
            .expect("inject");
        if let Some(directive) = sender.pending_encoder_directive() {
            assert!(sender.acknowledge_encoder_directive(directive.id, directive.target_height,));
            downshifted_480 = true;
            break;
        }
    }
    assert!(downshifted_480, "ABR must request second downshift to 480");
    assert_eq!(sender.bitrate_active_height(), 480);

    let (au_480, sps_480, pps_480) = encode_pattern(854, 480, 3);
    sender.set_stream_config(StreamConfigParams {
        width: 854,
        height: 480,
        fps: 30,
        bitrate_bps: 1_800_000,
        stream_epoch: 3,
        mirrored: false,
        rotation: 0,
        sps: sps_480,
        pps: pps_480,
    });
    for _ in 0..80 {
        receiver.pump().ok();
        sender.pump().ok();
        if sender.stream_config_sent() && receiver.stream_config().is_some_and(|c| c.height == 480)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        receiver.stream_config().map(|c| (c.width, c.height)),
        Some((854, 480)),
        "StreamConfig must be 854x480 after second ABR apply"
    );
    sender.ingest_and_flush(&au_480, true, 3, 3).expect("480");
    let mut ok480 = false;
    for _ in 0..400 {
        receiver.pump().ok();
        sender.pump().ok();
        if let Some(frame) = receiver.latest_frame() {
            if frame.width == 854 && frame.height == 480 {
                assert_eq!(frame.pixel_data.len(), nv12_byte_size(854, 480));
                ok480 = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(ok480, "FrameHub must show 480p post-downshift frames");
}

#[cfg(all(not(windows), not(target_vendor = "apple")))]
#[test]
fn abr_upshift_updates_stream_config_and_framehub() {
    // REQ-PICOO-MEDIA-010: after downshift, sustained health → UpshiftResolution → 1080p FrameHub.
    use openh264::encoder::Encoder;
    use openh264::formats::YUVBuffer;
    use picoo_frame_hub::nv12_byte_size;
    use picoo_packet::extract_sps_pps;
    use picoo_pairing::TrustedDevice;
    use picoo_protocol::control::ReceiverStats as ReceiverStatsMsg;
    use picoo_sender::StreamConfigParams;
    use picoo_session::ReceiverStatus;
    use picoo_transport::{Endpoint, QuicSenderTransport};
    use prost::Message;

    fn encode_pattern(w: usize, h: usize, seed: u8) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let mut planes = vec![128u8; w * h * 3 / 2];
        planes[..w * h].fill(seed.saturating_add(32));
        let yuv = YUVBuffer::from_vec(planes, w, h);
        let mut encoder = Encoder::new().expect("encoder");
        let annex = encoder.encode(&yuv).expect("encode").to_vec();
        let (sps, pps) = extract_sps_pps(&annex).expect("sps/pps");
        (annex, sps, pps)
    }

    let (au_lo, sps_lo, pps_lo) = encode_pattern(1280, 720, 3);
    let (au_hi, sps_hi, pps_hi) = encode_pattern(1920, 1080, 11);

    let mut receiver = ReceiverSession::new();
    receiver.set_jitter_target_ms(0);
    receiver.trusted_devices_mut().upsert(TrustedDevice {
        device_id: "abr-up-phone".into(),
        device_name: "AbrUp".into(),
        public_key: vec![6, 6, 6],
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
    sender.set_preferred_height(1080);
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
        .send_client_hello("abr-up-phone", "AbrUp", &[6, 6, 6])
        .expect("hello");
    for _ in 0..200 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if receiver.status() == ReceiverStatus::Streaming {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(receiver.status(), ReceiverStatus::Streaming);

    // Start at 1080 ladder then force congestion downshift (same as Android poll path).
    sender.set_stream_config(StreamConfigParams {
        width: 1920,
        height: 1080,
        fps: 30,
        bitrate_bps: 6_000_000,
        stream_epoch: 1,
        mirrored: false,
        rotation: 0,
        sps: sps_hi.clone(),
        pps: pps_hi.clone(),
    });
    for _ in 0..40 {
        receiver.pump().ok();
        sender.pump().ok();
        if sender.stream_config_sent() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    let mut downshifted = false;
    for _ in 0..40 {
        let stats = ReceiverStatsMsg {
            packet_loss: 0.05,
            frame_age_ms: 250.0,
            ..Default::default()
        };
        let mut buf = Vec::new();
        stats.encode(&mut buf).expect("encode");
        sender
            .inject_control_for_test(bytes::Bytes::from(buf))
            .expect("inject");
        if let Some(directive) = sender.pending_encoder_directive() {
            assert!(sender.acknowledge_encoder_directive(directive.id, directive.target_height,));
            downshifted = true;
            break;
        }
    }
    assert!(downshifted, "need downshift before upshift path");
    assert_eq!(sender.bitrate_active_height(), 720);

    sender.set_stream_config(StreamConfigParams {
        width: 1280,
        height: 720,
        fps: 30,
        bitrate_bps: 3_000_000,
        stream_epoch: 2,
        mirrored: false,
        rotation: 0,
        sps: sps_lo,
        pps: pps_lo,
    });
    for _ in 0..80 {
        receiver.pump().ok();
        sender.pump().ok();
        if receiver.stream_config().is_some_and(|c| c.height == 720) {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    sender.ingest_and_flush(&au_lo, true, 2, 2).expect("lo");
    for _ in 0..200 {
        receiver.pump().ok();
        sender.pump().ok();
        if receiver.latest_frame().is_some_and(|f| f.width == 1280) {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    // Climb 720 ladder then request upshift (rate-control needs near-max + 8 healthy ticks).
    let mut upshifted = false;
    for _ in 0..200 {
        let stats = ReceiverStatsMsg {
            packet_loss: 0.0,
            frame_age_ms: 40.0,
            jitter_buffer_depth_ms: 40.0,
            ..Default::default()
        };
        let mut buf = Vec::new();
        stats.encode(&mut buf).expect("encode");
        sender
            .inject_control_for_test(bytes::Bytes::from(buf))
            .expect("inject");
        if let Some(directive) = sender.pending_encoder_directive() {
            assert!(sender.acknowledge_encoder_directive(directive.id, directive.target_height,));
            upshifted = true;
            break;
        }
    }
    assert!(
        upshifted,
        "ABR must request resolution upshift after sustained health"
    );
    assert_eq!(sender.bitrate_active_height(), 1080);

    sender.set_stream_config(StreamConfigParams {
        width: 1920,
        height: 1080,
        fps: 30,
        bitrate_bps: 6_000_000,
        stream_epoch: 3,
        mirrored: false,
        rotation: 0,
        sps: sps_hi,
        pps: pps_hi,
    });
    for _ in 0..80 {
        receiver.pump().ok();
        sender.pump().ok();
        if receiver.stream_config().is_some_and(|c| c.height == 1080) {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        receiver.stream_config().map(|c| (c.width, c.height)),
        Some((1920, 1080)),
        "StreamConfig must return to 1920x1080 after ABR upshift"
    );
    sender.ingest_and_flush(&au_hi, true, 3, 3).expect("hi");
    let mut ok = false;
    for _ in 0..400 {
        receiver.pump().ok();
        sender.pump().ok();
        if let Some(frame) = receiver.latest_frame() {
            if frame.width == 1920 && frame.height == 1080 {
                assert_eq!(frame.pixel_data.len(), nv12_byte_size(1920, 1080));
                ok = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(ok, "FrameHub must show post-upshift frames");
}
