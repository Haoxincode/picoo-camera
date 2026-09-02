#[cfg(all(not(windows), not(target_vendor = "apple")))]
use std::time::Duration;

#[cfg(all(not(windows), not(target_vendor = "apple")))]
use picoo_sender::SenderSession;

#[cfg(all(not(windows), not(target_vendor = "apple")))]
use crate::ReceiverSession;

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
            jitter_buffer_occupancy_ms: 40.0,
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
