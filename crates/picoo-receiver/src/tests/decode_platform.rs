use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
use picoo_pairing::TrustedDevice;
use picoo_sender::SenderSession;
#[cfg(target_os = "macos")]
use picoo_session::ReceiverStatus;
#[cfg(target_os = "macos")]
use picoo_transport::{Endpoint, PicooTransport, QuicSenderTransport};

use crate::ReceiverSession;

#[cfg(all(not(windows), not(target_vendor = "apple")))]
#[test]
fn paired_openh264_access_unit_reaches_latest_frame_store() {
    // REQ-PICOO-MEDIA-005/006: real Annex-B H.264 through QUIC → decode → LatestFrameStore.
    use openh264::encoder::Encoder;
    use openh264::formats::YUVBuffer;
    use picoo_bitstream::avc::extract_sps_pps;
    #[cfg(not(any(target_os = "macos", windows)))]
    use picoo_frame_hub::nv12_byte_size;
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
    let mut encoder = Encoder::with_api_config(
        openh264::OpenH264API::from_source(),
        openh264::encoder::EncoderConfig::new()
            .profile(openh264::encoder::Profile::High)
            .vui(openh264::encoder::VuiConfig::bt709()),
    )
    .expect("openh264 encoder");
    let bitstream = encoder.encode(&yuv).expect("encode");
    let annex = bitstream.to_vec();
    assert!(annex.len() > 64, "AU too small for OpenH264 path");
    let (sps, pps) = extract_sps_pps(&annex).expect("SPS/PPS from Annex-B");
    let annex = super::wire_avc(&annex);

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
        if receiver.status() == ReceiverStatus::Streaming
            && matches!(
                sender.status(),
                picoo_session::SenderStatus::Streaming
                    | picoo_session::SenderStatus::NetworkUnstable
            )
        {
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
        configuration: picoo_bitstream::CodecConfiguration::from_avc_parameter_sets(&sps, &pps)
            .unwrap()
            .into(),
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
            if super::source_dimensions(frame) == (width as u32, height as u32) {
                assert_eq!(
                    frame.pixel_data.len(),
                    nv12_byte_size(frame.width, frame.height)
                );
                assert!(
                    frame.pixel_data.iter().any(|b| *b != 16 && *b != 128),
                    "expected non-placeholder NV12 from OpenH264"
                );
                assert_eq!(receiver.ingress_stats().decode_invocations, 1);
                assert_eq!(receiver.ingress_stats().access_units, 1);
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!(
        "OpenH264 AU did not reach LatestFrameStore at {}x{}; stats={:?}",
        width,
        height,
        receiver.ingress_stats()
    );
}

#[test]
fn paired_avcc_length_prefixed_au_reaches_latest_frame_store() {
    // REQ-PICOO-PROTOCOL-005 / MEDIA-005: MediaCodec-shaped AVCC AU reaches the
    // platform decoder. The encoded fixture avoids a test-only native codec.
    use picoo_bitstream::avc::{
        annex_b_to_length_prefixed, extract_sps_pps, is_length_prefixed_access_unit,
    };
    #[cfg(not(any(target_os = "macos", windows)))]
    use picoo_frame_hub::nv12_byte_size;
    use picoo_pairing::TrustedDevice;
    use picoo_sender::StreamConfigParams;
    use picoo_session::ReceiverStatus;
    use picoo_testkit::AVC_64X64_BT709_IDR as H264_64X64_RED_IDR;
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
        configuration: picoo_bitstream::CodecConfiguration::from_avc_parameter_sets(&sps, &pps)
            .unwrap()
            .into(),
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
    // short sequence still verifies the live AVCC -> MF -> LatestFrameStore path without
    // relying on a drain operation that production streaming never performs.
    for pts_us in 1..=3 {
        sender
            .ingest_and_flush(&avcc, true, pts_us, 1)
            .expect("ingest avcc");
        let decode_deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < decode_deadline {
            receiver.pump().ok();
            sender.pump().ok();
            if let Some(frame) = receiver.latest_frame() {
                if super::source_dimensions(frame) == (width as u32, height as u32) {
                    #[cfg(not(any(target_os = "macos", windows)))]
                    {
                        assert_eq!(
                            frame.pixel_data.len(),
                            nv12_byte_size(frame.width, frame.height)
                        );
                        assert!(frame.pixel_data.iter().any(|b| *b != 16 && *b != 128));
                    }
                    #[cfg(any(target_os = "macos", windows))]
                    assert_eq!(frame.description().visible_rect.width, width as u32);
                    return;
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    panic!(
        "AVCC AU did not reach LatestFrameStore; stats={:?}; media_error={:?}",
        receiver.ingress_stats(),
        receiver.last_media_error()
    );
}

#[cfg(target_os = "macos")]
#[test]
fn macos_videotoolbox_explicit_source_configuration() {
    // REQ-PICOO-MEDIA-027/028: explicit epoch changes flow through QUIC and
    // rebuild VideoToolbox with the dimensions advertised by StreamConfig.
    use picoo_bitstream::avc::extract_sps_pps;
    #[cfg(not(any(target_os = "macos", windows)))]
    use picoo_frame_hub::nv12_byte_size;
    use picoo_protocol::control::ReceiverStats as ReceiverStatsMsg;
    use picoo_sender::StreamConfigParams;
    use picoo_testkit::{
        AVC_1280X720_BT709_IDR as H264_1280X720_RED_IDR,
        AVC_1920X1080_BT709_IDR as H264_1920X1080_RED_IDR,
    };

    let mut receiver = ReceiverSession::new();
    receiver.set_jitter_target_ms(0);
    receiver.trusted_devices_mut().upsert(TrustedDevice {
        device_id: "macos-source-phone".into(),
        device_name: "macOS Source".into(),
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
        if receiver.status() == ReceiverStatus::Streaming
            && sender.status() == picoo_session::SenderStatus::Streaming
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(receiver.status(), ReceiverStatus::Streaming);
    assert_eq!(sender.status(), picoo_session::SenderStatus::Streaming);

    let check_congestion = |sender: &mut SenderSession<QuicSenderTransport>| {
        let epoch = sender.current_stream_epoch();
        let height = sender.bitrate_active_height();
        for _ in 0..1000 {
            sender.apply_receiver_stats_for_test(ReceiverStatsMsg {
                packet_loss: 0.05,
                frame_age_ms: 250.0,
                ..Default::default()
            });
            assert!(sender.pending_encoder_directive().is_none());
            assert_eq!(sender.current_stream_epoch(), epoch);
            assert_eq!(sender.bitrate_active_height(), height);
        }
    };

    check_congestion(&mut sender);
    let epoch_720 = sender.begin_stream_reconfiguration(picoo_sender::SourceFormat {
        codec: picoo_bitstream::Codec::Avc,
        height: 720,
        fps: 30,
    });
    assert_eq!(epoch_720, 2);
    let transaction_720 = sender.encoder_transaction_id_for_epoch(epoch_720);
    let (sps_720, pps_720) = extract_sps_pps(H264_1280X720_RED_IDR).expect("720p parameter sets");
    sender.set_stream_config(StreamConfigParams {
        width: 1280,
        height: 720,
        fps: 30,
        bitrate_bps: 3_000_000,
        stream_epoch: epoch_720,
        configuration: picoo_bitstream::CodecConfiguration::from_avc_parameter_sets(
            &sps_720, &pps_720,
        )
        .unwrap()
        .into(),
        mirrored: false,
        rotation: 0,
    });
    assert!(sender.report_encoder_started(transaction_720, 2, epoch_720, 720,));
    sender
        .ingest_encoder_access_unit(super::native_au(
            &super::wire_avc(H264_1280X720_RED_IDR),
            true,
            2,
            (transaction_720, 2, epoch_720, 720),
        ))
        .unwrap_or_else(|error| {
            panic!(
                "commit and queue 720p IDR: {error:?} (status {:?})",
                sender.status()
            )
        });
    sender.flush_pending().expect("send 720p IDR");
    assert_eq!(sender.bitrate_active_height(), 720);
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
    // A cold hardware VideoToolbox session can take well above 15 seconds on
    // a busy macOS CI host even though steady-state submissions are fast.
    let decode_deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < decode_deadline {
        receiver.pump().expect("receiver 720p frame");
        sender.pump().ok();
        if receiver
            .latest_frame()
            .is_some_and(|frame| super::source_dimensions(frame) == (1280, 720))
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let frame_720 = receiver.latest_frame().unwrap_or_else(|| {
        panic!(
            "720p frame missing: config {:?}, ingress {:?}, sender {:?}, link {:?}, media_error {:?}",
            receiver.stream_config().map(|config| (
                config.stream_epoch,
                config.width,
                config.height
            )),
            receiver.ingress_stats(),
            sender.stats(),
            sender.transport().link_stats(),
            receiver.last_media_error(),
        )
    });
    assert_eq!(super::source_dimensions(frame_720), (1280, 720));
    assert_eq!(frame_720.identity().stream_epoch, 2);
    let revision_720 = frame_720.description().config_revision;

    check_congestion(&mut sender);
    let epoch_1080 = sender.begin_stream_reconfiguration(picoo_sender::SourceFormat {
        codec: picoo_bitstream::Codec::Avc,
        height: 1080,
        fps: 30,
    });
    assert_eq!(epoch_1080, 3);
    let transaction_1080 = sender.encoder_transaction_id_for_epoch(epoch_1080);
    let (sps_1080, pps_1080) =
        extract_sps_pps(H264_1920X1080_RED_IDR).expect("1080p parameter sets");
    sender.set_stream_config(StreamConfigParams {
        width: 1920,
        height: 1080,
        fps: 30,
        bitrate_bps: 6_000_000,
        stream_epoch: epoch_1080,
        configuration: picoo_bitstream::CodecConfiguration::from_avc_parameter_sets(
            &sps_1080, &pps_1080,
        )
        .unwrap()
        .into(),
        mirrored: false,
        rotation: 0,
    });
    assert!(sender.report_encoder_started(transaction_1080, 3, epoch_1080, 1080,));
    sender
        .ingest_encoder_access_unit(super::native_au(&super::wire_avc(H264_1920X1080_RED_IDR), true, 3, (transaction_1080, 3, epoch_1080, 1080)))
        .unwrap_or_else(|error| {
            panic!(
                "commit and queue 1080p IDR: {error:?} (sender {:?}, receiver {:?}, session_error {:?})",
                sender.status(),
                receiver.status(),
                sender.last_session_error(),
            )
        });
    sender.flush_pending().expect("send 1080p IDR");
    assert_eq!(sender.bitrate_active_height(), 1080);
    for _ in 0..80 {
        receiver.pump().expect("receiver 1080p config");
        sender.pump().expect("sender 1080p config");
        if receiver
            .stream_config()
            .is_some_and(|config| config.stream_epoch == 3)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let decode_deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < decode_deadline {
        receiver.pump().expect("receiver 1080p frame");
        sender.pump().ok();
        if receiver.latest_frame().is_some_and(|frame| {
            frame.description().config_revision > revision_720
                && super::source_dimensions(frame) == (1920, 1080)
        }) {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let frame_1080 = receiver.latest_frame().expect("1080p frame");
    assert!(frame_1080.description().config_revision > revision_720);
    assert_eq!(super::source_dimensions(frame_1080), (1920, 1080));
    assert_eq!(frame_1080.identity().stream_epoch, 3);
    assert_eq!(frame_1080.description().coded_size.height, 1088);
    assert_eq!(frame_1080.description().visible_rect.height, 1080);
}

#[cfg(not(windows))]
#[test]
fn thermal_hold_changes_bitrate_growth_without_source_reconfiguration() {
    use picoo_protocol::control::ReceiverStats as ReceiverStatsMsg;
    use picoo_transport::QuicSenderTransport;
    let mut sender = SenderSession::new(QuicSenderTransport::new());
    sender.set_stream_config(picoo_sender::StreamConfigParams {
        width: 1280,
        height: 720,
        ..super::configured_source()
    });
    assert!(sender.report_encoder_started(0, 1, sender.current_stream_epoch(), 720));
    sender.set_thermal_hold(true);
    let epoch = sender.current_stream_epoch();
    let initial = sender.current_bitrate_bps();
    for _ in 0..80 {
        sender.apply_receiver_stats_for_test(ReceiverStatsMsg::default());
        assert!(sender.pending_encoder_directive().is_none());
        assert_eq!(sender.current_bitrate_bps(), initial);
    }
    sender.set_thermal_hold(false);
    for _ in 0..120 {
        sender.apply_receiver_stats_for_test(ReceiverStatsMsg::default());
        assert!(sender.pending_encoder_directive().is_none());
        assert_eq!(sender.current_stream_epoch(), epoch);
        assert_eq!(sender.bitrate_active_height(), 720);
    }
    assert!(sender.current_bitrate_bps() > initial);
}

#[cfg(all(not(windows), not(target_vendor = "apple")))]
#[test]
fn paired_openh264_publishes_to_shared_frame_ring() {
    // REQ-PICOO-FRAME-003 / VCAM-003: decode once → Shared Frame Ring for VCam consumer.
    use openh264::encoder::Encoder;
    use openh264::formats::YUVBuffer;
    use picoo_bitstream::avc::extract_sps_pps;
    use picoo_frame_hub::{
        nv12_byte_size, SharedFrameRingConsumer, SharedFrameRingProducer, DEFAULT_MAX_FRAME_BYTES,
    };
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
    let mut encoder = Encoder::with_api_config(
        openh264::OpenH264API::from_source(),
        openh264::encoder::EncoderConfig::new()
            .profile(openh264::encoder::Profile::High)
            .vui(openh264::encoder::VuiConfig::bt709()),
    )
    .expect("openh264 encoder");
    let annex = encoder.encode(&yuv).expect("encode").to_vec();
    let (sps, pps) = extract_sps_pps(&annex).expect("SPS/PPS");
    let annex = super::wire_avc(&annex);

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
    // The ring is an asynchronous output adapter, so attachment must not make
    // the Receiver owner wait for the initial placeholder copy.
    let placeholder_deadline = std::time::Instant::now() + Duration::from_secs(1);
    let placeholder = loop {
        if let Some(frame) = consumer.latest_frame() {
            break frame;
        }
        assert!(
            std::time::Instant::now() < placeholder_deadline,
            "asynchronous ring writer did not publish placeholder"
        );
        std::thread::sleep(Duration::from_millis(1));
    };
    assert!(placeholder.sequence >= 1);
    let placeholder_seq = placeholder.sequence;

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
        rotation: 90,
        configuration: picoo_bitstream::CodecConfiguration::from_avc_parameter_sets(&sps, &pps)
            .unwrap()
            .into(),
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
        receiver.ingress_stats()
    );
}
