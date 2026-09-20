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
    assert!(session.current_bitrate_bps() < BitrateLadder::for_height(1080).unwrap().initial_bps);
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
fn sustained_network_feedback_never_creates_a_source_configuration_transaction() {
    let mut session = SenderSession::new(MemoryTransport::new());
    let epoch = session.current_stream_epoch();
    let next_transaction = session.next_encoder_directive_id;
    let initial = session.current_bitrate_bps();
    for _ in 0..1000 {
        session.apply_receiver_stats_for_test(ReceiverStatsMsg {
            packet_loss: 0.2,
            frame_age_ms: 500.0,
            ..Default::default()
        });
        assert!(session.pending_encoder_directive().is_none());
        assert_eq!(session.current_stream_epoch(), epoch);
        assert_eq!(session.bitrate_active_height(), 1080);
    }
    assert!(session.current_bitrate_bps() < initial);
    for _ in 0..1000 {
        session.apply_receiver_stats_for_test(ReceiverStatsMsg::default());
        assert!(session.pending_encoder_directive().is_none());
        assert_eq!(session.current_stream_epoch(), epoch);
        assert_eq!(session.bitrate_active_height(), 1080);
    }
    assert_eq!(session.next_encoder_directive_id, next_transaction);
}

#[test]
fn rejected_explicit_configuration_can_be_requested_again_without_automatic_retry() {
    let mut session = SenderSession::new(MemoryTransport::new());
    let first_epoch = session.begin_stream_reconfiguration(crate::SourceFormat {
        codec: picoo_bitstream::Codec::Avc,
        height: 720,
        fps: 30,
    });
    let first_id = session.encoder_transaction_id_for_epoch(first_epoch);
    assert_ne!(first_id, 0);
    assert_eq!(
        session.report_encoder_failed(first_id, 0),
        EncoderFailureOutcome::RolledBack
    );
    assert_eq!(session.bitrate_active_height(), 1080);
    assert!(!session.encoder_apply_state.is_applying());
    let retry_epoch = session.begin_stream_reconfiguration(crate::SourceFormat {
        codec: picoo_bitstream::Codec::Avc,
        height: 720,
        fps: 30,
    });
    assert!(retry_epoch > first_epoch);
    assert_ne!(
        session.encoder_transaction_id_for_epoch(retry_epoch),
        first_id
    );
    assert_eq!(
        session.begin_stream_reconfiguration(crate::SourceFormat {
            codec: picoo_bitstream::Codec::Avc,
            height: 1080,
            fps: 30
        }),
        0
    );
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
