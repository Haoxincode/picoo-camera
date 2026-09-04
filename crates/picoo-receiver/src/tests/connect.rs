use std::time::Duration;

use picoo_pairing::TrustedDevice;
use picoo_sender::SenderSession;
use picoo_session::{ReceiverStatus, SenderStatus};
use picoo_transport::{Endpoint, QuicSenderTransport};

use crate::ReceiverSession;

use super::use_stub_decoder;

#[test]
fn paired_connect_to_streaming_under_three_seconds() {
    // PUC-002 / REQ-PICOO-DISCOVERY-006: paired connect establish < 3s (QUIC hello→Streaming).
    use std::time::Instant;

    let mut samples_ms = Vec::new();
    for round in 0..5u32 {
        let mut receiver = ReceiverSession::new();
        let bind = receiver
            .listen(Endpoint {
                host: "127.0.0.1".into(),
                port: 0,
            })
            .expect("listen");
        let mut sender = SenderSession::new(QuicSenderTransport::new());
        super::trust_receiver(&mut sender, &mut receiver);
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
        sender.send_client_hello().expect("hello");
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
    super::trust_receiver(&mut sender, &mut receiver);
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
    sender.send_client_hello().expect("hello");
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
    super::trust_receiver(&mut sender, &mut receiver);
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
    sender.send_client_hello().expect("hello");
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
    use picoo_transport::{Endpoint, QuicSenderTransport};

    let mut receiver = ReceiverSession::new();
    receiver.set_jitter_target_ms(0);
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
        .expect("connect from manual endpoint");
    // Android submits its stable identity immediately; QUIC is still connecting here.
    sender
        .send_client_hello()
        .expect("queue hello while connecting");
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
    super::trust_receiver(&mut sender, &mut receiver);
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
    sender.send_client_hello().expect("hello");
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
