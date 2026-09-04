use picoo_rate_control::BitrateLadder;

use super::*;

#[test]
fn receiver_stats_adjusts_bitrate() {
    let mut session = SenderSession::new(MemoryTransport::new());
    let endpoint = Endpoint {
        host: "127.0.0.1".into(),
        port: 4433,
    };
    session.connect(endpoint).expect("connect");

    let stats = ReceiverStatsMsg {
        packet_loss: 0.05,
        ..Default::default()
    };
    session.apply_receiver_stats_for_test(stats);
    session.pump().expect("pump");
    assert_eq!(session.last_bitrate_action(), BitrateAction::Decrease);
    assert!(session.current_bitrate_bps() < BitrateLadder::for_height(1080).initial_bps);
}

#[test]
fn sustained_floor_congestion_requests_resolution_downshift() {
    let mut session = SenderSession::new(MemoryTransport::new());
    session
        .connect(Endpoint {
            host: "127.0.0.1".into(),
            port: 4433,
        })
        .expect("connect");
    // Drive bitrate to the floor first.
    for _ in 0..20 {
        let stats = ReceiverStatsMsg {
            packet_loss: 0.05,
            frame_age_ms: 250.0,
            ..Default::default()
        };
        session.apply_receiver_stats_for_test(stats);
    }
    // Keep injecting while at floor until downshift fires.
    let mut saw = false;
    for _ in 0..10 {
        let stats = ReceiverStatsMsg {
            packet_loss: 0.05,
            frame_age_ms: 250.0,
            ..Default::default()
        };
        session.apply_receiver_stats_for_test(stats);
        if session.pending_encoder_directive().is_some() {
            saw = true;
            break;
        }
    }
    assert!(
        saw,
        "expected resolution downshift after sustained floor congestion"
    );
    let directive = session.pending_encoder_directive().expect("directive");
    assert_eq!(directive.kind, EncoderDirectiveKind::AbrDownshift);
    assert_eq!(directive.target_height, 720);
    assert_eq!(session.bitrate_active_height(), 1080);
    assert_eq!(session.pending_encoder_directive(), Some(directive));
    assert!(session.acknowledge_encoder_directive(directive.id, 720));
    assert_eq!(session.bitrate_active_height(), 720);
    assert!(session.pending_encoder_directive().is_none());
}

#[test]
fn rejected_encoder_directive_keeps_active_height_and_can_retry() {
    let mut session = SenderSession::new(MemoryTransport::new());
    session
        .connect(Endpoint {
            host: "127.0.0.1".into(),
            port: 4433,
        })
        .expect("connect");
    for _ in 0..30 {
        let stats = ReceiverStatsMsg {
            packet_loss: 0.05,
            frame_age_ms: 250.0,
            ..Default::default()
        };
        session.apply_receiver_stats_for_test(stats);
        if session.pending_encoder_directive().is_some() {
            break;
        }
    }
    let first = session
        .pending_encoder_directive()
        .expect("first directive");
    assert!(session.reject_encoder_directive(first.id));
    assert_eq!(session.bitrate_active_height(), 1080);

    for _ in 0..10 {
        let stats = ReceiverStatsMsg {
            packet_loss: 0.05,
            frame_age_ms: 250.0,
            ..Default::default()
        };
        session.apply_receiver_stats_for_test(stats);
        if session.pending_encoder_directive().is_some() {
            break;
        }
    }
    let retry = session
        .pending_encoder_directive()
        .expect("retry directive");
    assert_ne!(retry.id, first.id);
    assert_eq!(retry.target_height, 720);
    assert_eq!(session.bitrate_active_height(), 1080);

    assert_eq!(session.begin_stream_reconfiguration(), 0);
    assert_eq!(session.pending_encoder_directive(), Some(retry));
    assert!(session.reject_encoder_directive(retry.id));
    let local_epoch = session.begin_stream_reconfiguration();
    assert!(local_epoch > retry.stream_epoch);
    assert_eq!(session.begin_stream_reconfiguration(), 0);
    assert_eq!(session.bitrate_active_height(), 1080);
}

#[test]
fn encoder_command_request_keyframe_sets_flag() {
    use picoo_protocol::control::encoder_command;
    use picoo_protocol::control::EncoderCommand;

    let mut session = SenderSession::new(MemoryTransport::new());
    session
        .connect(Endpoint {
            host: "127.0.0.1".into(),
            port: 1,
        })
        .expect("connect");
    session
        .send_client_hello("sender", "Phone", &[1, 2, 3])
        .expect("hello");
    session
        .trusted_devices_mut()
        .upsert(picoo_pairing::TrustedDevice {
            device_id: "receiver".into(),
            device_name: "Receiver".into(),
            public_key: vec![4, 5, 6],
            certificate_fingerprint: "test".into(),
            paired_at_ms: 1,
            last_connected_at_ms: None,
        });
    session
        .inject_control_payload_for_test(ControlPayload::ServerHello(ServerHello {
            receiver_id: "receiver".into(),
            display_name: "Receiver".into(),
            public_key: vec![4, 5, 6],
            pairing_required: false,
        }))
        .expect("authenticated server hello");
    assert!(session.take_keyframe_request());
    let cmd = EncoderCommand {
        command: encoder_command::Command::RequestKeyframe as i32,
    };
    session
        .inject_control_payload_for_test(ControlPayload::EncoderCommand(cmd))
        .expect("inject");
    assert!(session.take_keyframe_request());
    assert!(!session.take_keyframe_request());
}
