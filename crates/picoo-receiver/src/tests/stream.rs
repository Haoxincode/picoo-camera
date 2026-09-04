use std::time::Duration;

use picoo_pairing::TrustedDevice;
use picoo_sender::SenderSession;
use picoo_session::{ReceiverStatus, SenderStatus};
use picoo_transport::{Endpoint, QuicSenderTransport};

use crate::ReceiverSession;

use super::use_stub_decoder;

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
    super::trust_receiver(&mut sender, &receiver);
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
    assert_eq!(receiver.last_stats_revision(), 1);
    assert!(stats.receive_bitrate > 0);
    // One idle stats interval produces a stale frame, but ARCH-PICOO-SESSION-001
    // requires sustained age growth before sacrificing quality.
    assert!(stats.frame_age_ms > 200.0);
    // REQ-PICOO-PROTOCOL-006: RTT comes from transport link stats (loopback ≥ 0).
    assert!(stats.rtt_ms >= 0.0);
    assert!(stats.rtt_ms < 5_000.0);
    // Healthy loopback should not report pathological loss.
    assert!(stats.packet_loss < 0.5);
    assert_eq!(sender.last_bitrate_action(), BitrateAction::Hold);

    // The revision identifies complete windows: pumps inside the same interval
    // do not advance it, and teardown clears current values without rewinding
    // the monotonic identity used by desktop history de-duplication.
    receiver.pump().expect("same-window pump");
    assert_eq!(receiver.last_stats_revision(), 1);
    receiver.close();
    assert!(receiver.last_stats().is_none());
    assert_eq!(receiver.decoded_fps(), 0);
    assert!(receiver.stream_config().is_none());
    assert_eq!(receiver.last_stats_revision(), 1);
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
    super::trust_receiver(&mut sender, &receiver);
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
    super::trust_receiver(&mut sender, &receiver);
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
    super::trust_receiver(&mut sender, &receiver);
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
    super::trust_receiver(&mut sender, &receiver);
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
