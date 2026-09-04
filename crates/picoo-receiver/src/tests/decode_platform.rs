use std::time::Duration;

#[cfg(target_os = "macos")]
use picoo_pairing::TrustedDevice;
use picoo_sender::SenderSession;
#[cfg(target_os = "macos")]
use picoo_session::ReceiverStatus;
#[cfg(target_os = "macos")]
use picoo_transport::{Endpoint, QuicSenderTransport};

use crate::ReceiverSession;

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

    sender.send_client_hello().expect("hello");
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
    super::trust_receiver(&mut sender, &mut receiver);
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
    sender.send_client_hello().expect("hello");
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
    // A synchronous MFT may legally retain the first sample while priming. A
    // short sequence still verifies the live AVCC -> MF -> FrameHub path without
    // relying on a drain operation that production streaming never performs.
    for pts_us in 1..=3 {
        sender
            .ingest_and_flush(&avcc, true, pts_us, 1)
            .expect("ingest avcc");
        for _ in 0..100 {
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
    }
    panic!(
        "AVCC AU did not reach FrameHub; stats={:?}; media_error={:?}",
        receiver.stats(),
        receiver.last_media_error()
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
    super::trust_receiver(&mut sender, &mut receiver);
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
    sender.send_client_hello().expect("hello");
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
            sender.apply_receiver_stats_for_test(stats);
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
            jitter_buffer_occupancy_ms: 40.0,
            ..Default::default()
        };
        sender.apply_receiver_stats_for_test(stats);
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
            jitter_buffer_occupancy_ms: 40.0,
            ..Default::default()
        };
        sender.apply_receiver_stats_for_test(stats);
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
    sender.send_client_hello().expect("hello");
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
