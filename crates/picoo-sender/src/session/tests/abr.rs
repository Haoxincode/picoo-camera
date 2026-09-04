use picoo_rate_control::BitrateLadder;

use super::*;

fn commit_directive(
    session: &mut SenderSession<MemoryTransport>,
    directive: EncoderDirective,
    generation: u64,
) {
    session.force_status_for_test(SenderStatus::Streaming);
    session.set_stream_config(StreamConfigParams {
        width: if directive.target_height == 1080 {
            1920
        } else {
            1280
        },
        height: directive.target_height,
        ..Default::default()
    });
    assert!(session.report_encoder_started(
        directive.id,
        generation,
        directive.stream_epoch,
        directive.target_height,
    ));
    session
        .ingest_encoder_access_unit(super::native_au(
            b"idr",
            true,
            1,
            (
                directive.id,
                generation,
                directive.stream_epoch,
                directive.target_height,
            ),
        ))
        .expect("matching IDR commits directive");
}

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
fn fec_tracks_frame_importance_and_receiver_loss() {
    let mut session = SenderSession::new(MemoryTransport::new());
    session
        .connect(Endpoint {
            host: "127.0.0.1".into(),
            port: 4433,
        })
        .expect("connect");
    session.force_status_for_test(SenderStatus::Streaming);
    let payload = vec![3_u8; picoo_protocol::MAX_FEC_FRAGMENT_PAYLOAD * 2];

    let healthy_delta = session
        .ingest_access_unit(&payload, false, 0, 1)
        .expect("healthy delta");
    assert_eq!(healthy_delta, 2);
    session.flush_pending().expect("flush healthy delta");

    let protected_idr = session
        .ingest_access_unit(&payload, true, 1, 1)
        .expect("protected IDR");
    assert_eq!(protected_idr, 4);
    session.flush_pending().expect("flush IDR");

    session.apply_receiver_stats_for_test(ReceiverStatsMsg {
        packet_loss: 0.0,
        pre_fec_packet_loss: 0.02,
        ..Default::default()
    });
    assert_eq!(session.last_bitrate_action(), BitrateAction::Hold);
    let light_delta = session
        .ingest_access_unit(&payload, false, 2, 1)
        .expect("light FEC delta");
    assert_eq!(light_delta, 3);
    session.flush_pending().expect("flush light FEC delta");

    session.apply_receiver_stats_for_test(ReceiverStatsMsg {
        packet_loss: 0.0,
        pre_fec_packet_loss: 0.04,
        ..Default::default()
    });
    let strong_delta = session
        .ingest_access_unit(&payload, false, 3, 1)
        .expect("strong FEC delta");
    assert_eq!(strong_delta, 4);
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
    commit_directive(&mut session, directive, 11);
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
    assert_eq!(
        session.report_encoder_failed(first.id, 0),
        EncoderFailureOutcome::RolledBack
    );
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

    let local_epoch = session.begin_stream_reconfiguration(720);
    assert!(local_epoch > retry.stream_epoch);
    assert!(session.pending_encoder_directive().is_none());
    assert_eq!(session.begin_stream_reconfiguration(720), 0);
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
    session.send_client_hello().expect("hello");
    let receiver = picoo_pairing::DeviceIdentity::generate("Receiver").expect("identity");
    authenticate_trusted_receiver(&mut session, &receiver);
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
