#[cfg(all(not(windows), not(target_vendor = "apple")))]
use std::time::Duration;

#[cfg(all(not(windows), not(target_vendor = "apple")))]
use picoo_sender::SenderSession;

#[cfg(all(not(windows), not(target_vendor = "apple")))]
use crate::ReceiverSession;

#[cfg(all(not(windows), not(target_vendor = "apple")))]
pub(super) fn openh264_au(width: usize, height: usize, seed: u8) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
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
fn stream_epoch_bump_recovers_openh264_latest_frame_store_under_three_seconds() {
    // PUC-005 / REQ-PICOO-MEDIA-003: camera/epoch switch → new IDR in LatestFrameStore <3s.
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
    let next_epoch = sender.begin_stream_reconfiguration(480);
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

    let transaction_id = sender.encoder_transaction_id_for_epoch(next_epoch);
    assert!(sender.report_encoder_started(transaction_id, 2, next_epoch, 480));
    sender
        .ingest_encoder_access_unit(super::native_au(
            &au2,
            true,
            2,
            (transaction_id, 2, next_epoch, 480),
        ))
        .expect("commit switched camera generation");
    sender.flush_pending().expect("send switched camera IDR");
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
fn midstream_resolution_change_openh264_updates_latest_frame_store() {
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
    sender.send_client_hello().expect("hello");
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
    let next_epoch = sender.begin_stream_reconfiguration(720);
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
    let transaction_id = sender.encoder_transaction_id_for_epoch(next_epoch);
    assert!(sender.report_encoder_started(transaction_id, 2, next_epoch, 720));
    sender
        .ingest_encoder_access_unit(super::native_au(
            &au_hi,
            true,
            2,
            (transaction_id, 2, next_epoch, 720),
        ))
        .expect("commit higher resolution generation");
    sender.flush_pending().expect("send higher resolution IDR");
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
    assert!(ok, "LatestFrameStore did not update to 1280x720");
    assert!(
        elapsed_ms < 3_000.0,
        "resolution switch {elapsed_ms}ms exceeds 3s budget"
    );
}

#[cfg(all(not(windows), not(target_vendor = "apple")))]
#[test]
fn incomplete_keyframe_requests_idr_and_recovers_latest_frame_store() {
    // REQ-PICOO-SESSION-003: incomplete IDR → RequestKeyframe → fresh IDR → LatestFrameStore.
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
    // and reports the loss exactly once. The recovery command also shares the
    // production one-second anti-storm window with the initial-config request.
    let mut keyed = false;
    let keyframe_deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < keyframe_deadline {
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
        "an incomplete IDR must not publish another LatestFrameStore frame"
    );

    // A fresh IDR on the current epoch recovers LatestFrameStore.
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
        "LatestFrameStore did not recover after RequestKeyframe IDR"
    );
}
