use super::*;

#[test]
fn invalid_configuration_cannot_send_control_or_commit_matching_idr() {
    // REQ-PICOO-PROTOCOL-019: a native refresh fact cannot make invalid
    // source attributes authoritative, even when its transaction identity matches.
    let mut session = SenderSession::new(MemoryTransport::new());
    session
        .connect(Endpoint {
            host: "127.0.0.1".into(),
            port: 4433,
        })
        .unwrap();
    session.force_status_for_test(SenderStatus::Streaming);
    session.set_stream_config(super::source_configuration(720));
    let committed_epoch = session.current_stream_epoch();
    assert!(session.report_encoder_started(0, 10, committed_epoch, 720));
    let candidate_epoch = session.begin_stream_reconfiguration(1080);
    let transaction = session.encoder_transaction_id_for_epoch(candidate_epoch);
    assert!(session.report_encoder_started(transaction, 11, candidate_epoch, 1080));
    for fps in [0, 120] {
        session.set_stream_config(StreamConfigParams {
            height: 1080,
            width: 1920,
            fps,
            ..super::source_configuration(1080)
        });
        let sent = session.transport.sent_order.len();
        let message_id = session.next_control_message_id;
        assert!(matches!(
            session.ingest_encoder_access_unit(super::native_au(
                b"matching-idr",
                true,
                3,
                (transaction, 11, candidate_epoch, 1080),
            )),
            Err(SenderError::CodecConfiguration(_))
        ));
        assert_eq!(session.current_stream_epoch(), committed_epoch);
        assert_eq!(
            session.encoder_transaction_id_for_epoch(candidate_epoch),
            transaction
        );
        assert_eq!(session.pending_packets(), 0);
        assert_eq!(session.transport.sent_order.len(), sent);
        assert_eq!(session.next_control_message_id, message_id);
    }
    session.set_stream_config(super::source_configuration(1080));
    session
        .ingest_encoder_access_unit(super::native_au(
            b"valid-configuration-idr",
            true,
            4,
            (transaction, 11, candidate_epoch, 1080),
        ))
        .expect("valid replacement can commit the pending transaction");
    assert_eq!(session.current_stream_epoch(), candidate_epoch);
}

#[test]
fn invalid_initial_encoder_event_does_not_bind_generation_or_stage_source() {
    let mut session = SenderSession::new(MemoryTransport::new());
    let epoch = session.current_stream_epoch();
    let outcome = session.submit_encoder_event(crate::NativeEncoderEvent {
        data: b"initial-idr",
        is_keyframe: true,
        pts_us: 1,
        encoded_at_us: 2,
        encoder_generation: 10,
        stream_epoch: epoch,
        width: 1280,
        height: 720,
        stream_config: Some(StreamConfigParams {
            fps: 0,
            ..super::source_configuration(720)
        }),
    });
    assert!(matches!(outcome, Err(SenderError::CodecConfiguration(_))));
    assert_eq!(session.committed_encoder_generation, 0);
    assert_eq!(session.committed_encoder_height, 0);
    assert!(session.pending_stream_config().is_none());
    assert_eq!(session.current_stream_epoch(), epoch);
    assert_eq!(session.pending_packets(), 0);
}
