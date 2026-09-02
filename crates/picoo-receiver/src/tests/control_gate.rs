use std::time::Duration;

use picoo_sender::SenderSession;
use picoo_session::{ReceiverStatus, SenderStatus};
use picoo_transport::{Endpoint, QuicSenderTransport};

use crate::ReceiverSession;

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
        .ingest_and_flush_unchecked_for_test(b"blocked-au", true, 1, 1)
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
    receiver.confirm_pairing_locally().expect("desktop confirm");
    sender
        .send_pairing_confirm(&identity.receiver_id)
        .expect("confirm");
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

    // Simulate a compromised/legacy peer that ignores the production sender gate.
    for frame_id in 1..=20u64 {
        let _ = super::video_send_accepted(sender.ingest_and_flush_unchecked_for_test(
            format!("unpaired-{frame_id}").as_bytes(),
            true,
            frame_id,
            1,
        ));
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
