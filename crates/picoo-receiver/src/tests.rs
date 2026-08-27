use std::time::Duration;

use picoo_pairing::TrustedDevice;
use picoo_sender::SenderSession;
use picoo_session::ReceiverStatus;
use picoo_transport::{Endpoint, QuicSenderTransport};

use crate::{run_loopback_access_unit, ReceiverSession};

#[test]
fn loopback_sender_to_receiver_frame_hub() {
    let frame = run_loopback_access_unit(b"test-access-unit").expect("loopback");
    assert_eq!(frame.as_ref(), b"test-access-unit");
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
    receiver.pump().expect("receiver pump");

    assert_eq!(receiver.stats().access_units, 0);
    assert!(receiver.stats().packets_dropped_unpaired > 0);
    assert_eq!(receiver.status(), ReceiverStatus::Connecting);
}

#[test]
fn paired_sender_enters_streaming_after_client_hello() {
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
fn first_time_pairing_flow_enables_video() {
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
    receiver.pump().expect("receiver pump");
    assert_eq!(receiver.stats().access_units, 1);
}

#[test]
fn receiver_sends_stats_to_paired_sender() {
    use picoo_sender::BitrateAction;
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

    sender
        .ingest_and_flush(&[0u8; 1200], true, 1, 1)
        .expect("send video");
    receiver.pump().expect("receiver pump");

    std::thread::sleep(Duration::from_millis(1100));

    for _ in 0..20 {
        receiver.pump().expect("receiver pump");
        sender.pump().expect("sender pump");
        if sender.last_receiver_stats().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    assert!(sender.last_receiver_stats().is_some());
    assert!(sender.last_receiver_stats().unwrap().receive_bitrate > 0);
    assert_eq!(sender.last_bitrate_action(), BitrateAction::Hold);
}
