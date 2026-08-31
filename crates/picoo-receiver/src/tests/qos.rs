use std::time::Duration;

use picoo_sender::SenderSession;
use picoo_session::{ReceiverStatus, SenderStatus};
use picoo_transport::{Endpoint, QuicSenderTransport};

use crate::ReceiverSession;

#[cfg(all(not(windows), not(target_vendor = "apple")))]
use super::abr_epoch::openh264_au;
use super::use_stub_decoder;

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
    receiver.confirm_pairing_locally().expect("desktop confirm");
    sender
        .send_pairing_confirm(&identity.receiver_id)
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
        if receiver.status() == ReceiverStatus::Streaming
            && sender.status() == SenderStatus::Streaming
        {
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
        if receiver.status() == ReceiverStatus::Streaming
            && sender.status() == SenderStatus::Streaming
        {
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
