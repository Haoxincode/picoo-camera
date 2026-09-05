use std::time::Duration;

use picoo_pairing::TrustedDevice;
use picoo_sender::SenderSession;
use picoo_session::{ReceiverStatus, SenderStatus};
use picoo_transport::{Endpoint, QuicSenderTransport};

use crate::ReceiverSession;

use super::use_stub_decoder;

#[test]
fn session_status_markers_cover_vcam_permission_and_network() {
    // REQ-PICOO-SESSION-001
    let mut receiver = ReceiverSession::new();
    assert_eq!(receiver.status(), ReceiverStatus::Disconnected);

    receiver.mark_permission_required();
    assert_eq!(receiver.status(), ReceiverStatus::PermissionRequired);
    receiver.mark_virtual_camera_unavailable();
    assert_eq!(
        receiver.status(),
        ReceiverStatus::PermissionRequired,
        "an output diagnostic must not erase the permission gate"
    );

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

    // Network health cannot overwrite a non-streaming lifecycle state.
    receiver.observe_network_packet_loss_for_test(0.05);
    receiver.observe_network_packet_loss_for_test(0.05);
    assert_eq!(receiver.status(), ReceiverStatus::Discovering);
}

#[test]
fn network_episode_is_hysteretic_without_overwriting_streaming_lifecycle() {
    // REQ-PICOO-SESSION-013
    let mut receiver = ReceiverSession::new();
    receiver.begin_streaming(picoo_transport::SessionId(1));

    receiver.observe_network_packet_loss_for_test(0.05);
    assert_eq!(receiver.status(), ReceiverStatus::Streaming);
    receiver.observe_network_packet_loss_for_test(0.04);
    assert_eq!(receiver.status(), ReceiverStatus::NetworkUnstable);
    assert_eq!(
        receiver.lifecycle_status_for_test(),
        ReceiverStatus::Streaming,
        "health must not replace the media lifecycle fact"
    );
    assert_eq!(
        receiver
            .network_health()
            .episode()
            .expect("episode")
            .worst_packet_loss,
        0.05
    );

    for _ in 0..4 {
        receiver.observe_network_packet_loss_for_test(0.0);
        assert_eq!(receiver.status(), ReceiverStatus::NetworkUnstable);
    }
    receiver.observe_network_packet_loss_for_test(0.0);
    assert_eq!(receiver.status(), ReceiverStatus::Streaming);
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
    let black_y_plane = &black[..1280 * 720];
    assert!(
        black_y_plane.iter().all(|&value| value == 0),
        "Black mode must publish a black Y plane"
    );
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

    sender.send_client_hello().expect("hello");
    for _ in 0..200 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if receiver.pairing_short_code().is_some() && sender.pairing_short_code().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    receiver.confirm_pairing_locally().expect("desktop confirm");
    sender
        .send_pairing_confirm(identity.receiver_id())
        .expect("confirm");
    for _ in 0..200 {
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
    super::trust_receiver(&mut sender, &mut receiver);
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

    sender.send_client_hello().expect("hello");
    for _ in 0..100 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if receiver.status() == ReceiverStatus::Streaming
            && sender.status() == SenderStatus::Streaming
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    sender
        .ingest_and_flush(b"jitter-hold-au", true, 1, 1)
        .expect("ingest");
    receiver.pump().expect("rx");
    // Immediately after first pump the AU should still be in the jitter buffer.
    assert_eq!(receiver.ingress_stats().access_units, 0);

    let started = std::time::Instant::now();
    while started.elapsed() < Duration::from_millis(200) {
        receiver.pump().expect("rx");
        if receiver.ingress_stats().access_units > 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(receiver.ingress_stats().access_units, 1);
    assert!(
        started.elapsed() >= Duration::from_millis(20),
        "expected adaptive startup hold near 33ms, released too early: {:?}",
        started.elapsed()
    );
}
