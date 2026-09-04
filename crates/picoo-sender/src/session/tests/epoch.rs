use super::*;

#[test]
fn stale_access_unit_epoch_is_rejected_after_reconfiguration_begins() {
    let mut session = SenderSession::new(MemoryTransport::new());
    session
        .connect(Endpoint {
            host: "127.0.0.1".into(),
            port: 4433,
        })
        .expect("connect");
    session.force_status_for_test(SenderStatus::Streaming);
    session.set_stream_config(StreamConfigParams {
        width: 1920,
        height: 1080,
        ..Default::default()
    });
    let committed_epoch = session.current_stream_epoch();
    assert!(session.report_encoder_started(0, 10, committed_epoch, 1080));
    let pending_epoch = session.begin_stream_reconfiguration(720);
    let transaction_id = session.encoder_transaction_id_for_epoch(pending_epoch);
    assert_ne!(pending_epoch, committed_epoch);
    assert!(matches!(
        session.ingest_encoder_access_unit(super::native_au(
            b"old-generation",
            true,
            1,
            (0, 10, committed_epoch, 1080)
        )),
        Err(SenderError::EncoderRefreshPending)
    ));
    assert!(session.report_encoder_started(transaction_id, 11, pending_epoch, 720));
    assert!(matches!(
        session.ingest_encoder_access_unit(super::native_au(
            b"delta",
            false,
            2,
            (transaction_id, 11, pending_epoch, 720)
        )),
        Err(SenderError::EncoderRefreshPending)
    ));
    session.set_stream_config(StreamConfigParams {
        width: 1280,
        height: 720,
        ..Default::default()
    });
    session
        .ingest_encoder_access_unit(super::native_au(
            b"current-idr",
            true,
            3,
            (transaction_id, 11, pending_epoch, 720),
        ))
        .expect("matching IDR commits and enters packetization");
    assert_eq!(session.current_stream_epoch(), pending_epoch);
    assert!(matches!(
        session.ingest_encoder_access_unit(super::native_au(
            b"now-stale",
            true,
            4,
            (0, 10, committed_epoch, 1080)
        )),
        Err(SenderError::StaleEncoderFact)
    ));
}

#[test]
fn stream_config_epoch_changes_only_when_native_apply_commits() {
    let mut session = SenderSession::new(MemoryTransport::new());
    session
        .connect(Endpoint {
            host: "127.0.0.1".into(),
            port: 4433,
        })
        .expect("connect");
    session.force_status_for_test(SenderStatus::Streaming);
    session.stream_config_sent = true;
    let committed = session.current_stream_epoch();
    let pending = session.begin_stream_reconfiguration(720);
    assert_ne!(pending, committed);
    assert!(session.stream_config_sent());
    assert_eq!(
        session
            .pending_stream_config()
            .map(|config| config.stream_epoch),
        Some(committed)
    );

    session.set_stream_config(StreamConfigParams {
        width: 1280,
        height: 720,
        ..Default::default()
    });
    let transaction_id = session.encoder_transaction_id_for_epoch(pending);
    assert!(session.report_encoder_started(transaction_id, 11, pending, 720));
    assert!(matches!(
        session.ingest_encoder_access_unit(super::native_au(
            b"delta",
            false,
            1,
            (transaction_id, 11, pending, 720)
        )),
        Err(SenderError::EncoderRefreshPending)
    ));
    assert_eq!(session.current_stream_epoch(), committed);
    session
        .ingest_encoder_access_unit(super::native_au(
            b"idr",
            true,
            2,
            (transaction_id, 11, pending, 720),
        ))
        .expect("matching IDR commits");
    assert_eq!(
        session
            .pending_stream_config()
            .map(|config| config.stream_epoch),
        Some(pending)
    );
    assert!(!session.media_blocked_for_stream_config);
}

#[test]
fn committed_encoder_started_fact_is_idempotent() {
    let mut session = SenderSession::new(MemoryTransport::new());
    let epoch = session.current_stream_epoch();
    session.set_stream_config(StreamConfigParams {
        width: 1920,
        height: 1080,
        ..Default::default()
    });
    assert!(session.report_encoder_started(0, 10, epoch, 1080));
    assert!(session.report_encoder_started(0, 10, epoch, 1080));
    assert!(!session.report_encoder_started(0, 10, epoch, 720));
    assert!(!session.report_encoder_started(0, 11, epoch, 1080));
    assert_eq!(session.bitrate_active_height(), 1080);
}

#[test]
fn stream_epoch_exhausts_before_crossing_android_signed_range() {
    let mut session = SenderSession::new(MemoryTransport::new());
    session.last_allocated_stream_epoch = MAX_STREAM_EPOCH;
    assert_eq!(session.begin_stream_reconfiguration(720), 0);
    assert_eq!(session.current_stream_epoch(), INITIAL_STREAM_EPOCH);
    assert_eq!(session.last_session_error(), Some("STREAM_EPOCH_EXHAUSTED"));
}

#[test]
fn invalid_local_target_does_not_consume_transaction_or_epoch_identity() {
    let mut session = SenderSession::new(MemoryTransport::new());
    let last_epoch = session.last_allocated_stream_epoch;
    let next_transaction = session.next_encoder_directive_id;

    assert_eq!(session.begin_stream_reconfiguration(0), 0);
    assert_eq!(session.last_allocated_stream_epoch, last_epoch);
    assert_eq!(session.next_encoder_directive_id, next_transaction);
    assert!(session.pending_encoder_directive().is_none());
}

#[test]
fn receiver_capability_caps_preferred_height_in_rust() {
    let mut session = SenderSession::new(MemoryTransport::new());
    let capabilities = Capabilities {
        codecs: vec!["h264".into()],
        resolutions: vec![
            Resolution {
                width: 854,
                height: 480,
            },
            Resolution {
                width: 1280,
                height: 720,
            },
        ],
        fps: vec![30],
        front_camera: true,
        back_camera: true,
    };
    assert!(session.apply_capabilities_for_test(capabilities));
    session.set_preferred_height(1080);
    assert_eq!(session.receiver_max_height(), 720);
    assert_eq!(session.bitrate.preferred_height(), 720);

    let expanded = Capabilities {
        codecs: vec!["h264".into()],
        resolutions: vec![Resolution {
            width: 1920,
            height: 1080,
        }],
        fps: vec![30],
        front_camera: true,
        back_camera: true,
    };
    assert!(session.apply_capabilities_for_test(expanded));
    assert_eq!(session.bitrate.preferred_height(), 1080);
}

#[test]
fn matching_config_staged_during_apply_is_kept_for_new_epoch() {
    let mut session = SenderSession::new(MemoryTransport::new());
    session
        .connect(Endpoint {
            host: "127.0.0.1".into(),
            port: 4433,
        })
        .expect("connect");
    session.force_status_for_test(SenderStatus::Streaming);
    let pending = session.begin_stream_reconfiguration(720);
    session.set_stream_config(StreamConfigParams {
        width: 1280,
        height: 720,
        sps: vec![1, 2, 3],
        ..Default::default()
    });
    let transaction_id = session.encoder_transaction_id_for_epoch(pending);
    assert!(session.report_encoder_started(transaction_id, 11, pending, 720));
    session
        .ingest_encoder_access_unit(super::native_au(
            b"idr",
            true,
            1,
            (transaction_id, 11, pending, 720),
        ))
        .expect("matching IDR");
    let config = session.pending_stream_config().expect("staged config");
    assert_eq!(config.stream_epoch, pending);
    assert_eq!(config.sps, vec![1, 2, 3]);
    assert!(!session.media_blocked_for_stream_config);
}

#[test]
fn wrong_height_config_cannot_open_committed_epoch_media_gate() {
    let mut session = SenderSession::new(MemoryTransport::new());
    session
        .connect(Endpoint {
            host: "127.0.0.1".into(),
            port: 4433,
        })
        .expect("connect");
    session.force_status_for_test(SenderStatus::Streaming);
    let pending = session.begin_stream_reconfiguration(720);
    session.set_stream_config(StreamConfigParams {
        width: 1920,
        height: 1080,
        ..Default::default()
    });
    let transaction_id = session.encoder_transaction_id_for_epoch(pending);
    assert!(session.report_encoder_started(transaction_id, 11, pending, 720));
    assert!(matches!(
        session.ingest_encoder_access_unit(super::native_au(
            b"blocked",
            true,
            1,
            (transaction_id, 11, pending, 720)
        )),
        Err(SenderError::StreamConfigPending { .. })
    ));
    assert_eq!(session.current_stream_epoch(), INITIAL_STREAM_EPOCH);
    assert_eq!(
        session.encoder_transaction_id_for_epoch(pending),
        transaction_id
    );
}

#[test]
fn noncanonical_encoder_height_cannot_commit_ladder_epoch() {
    let mut session = SenderSession::new(MemoryTransport::new());
    let pending = session.begin_stream_reconfiguration(720);
    session.set_stream_config(StreamConfigParams {
        width: 1280,
        height: 800,
        ..Default::default()
    });
    let transaction_id = session.encoder_transaction_id_for_epoch(pending);
    assert!(!session.report_encoder_started(transaction_id, 11, pending, 800));
    assert_eq!(session.current_stream_epoch(), INITIAL_STREAM_EPOCH);
}

#[test]
fn failed_before_start_restores_committed_stream_config() {
    let mut session = SenderSession::new(MemoryTransport::new());
    session.stream_config_sent = true;
    let committed = session.pending_stream_config().cloned();
    let pending = session.begin_stream_reconfiguration(720);
    session.set_stream_config(StreamConfigParams {
        width: 854,
        height: 480,
        ..Default::default()
    });
    assert_eq!(session.pending_stream_config().map(|c| c.height), Some(480));
    let transaction_id = session.encoder_transaction_id_for_epoch(pending);
    assert_eq!(
        session.report_encoder_failed(transaction_id, 0),
        EncoderFailureOutcome::RolledBack
    );
    assert_eq!(session.pending_stream_config().cloned(), committed);
    assert!(session.stream_config_sent());
}

#[test]
fn disconnect_aborts_pending_local_and_directive_generations() {
    let mut session = SenderSession::new(MemoryTransport::new());
    assert_ne!(session.begin_stream_reconfiguration(720), 0);
    assert!(session.encoder_apply_state.is_applying());
    assert!(session.pending_encoder_directive().is_none());
    session.disconnect();
    assert!(!session.encoder_apply_state.is_applying());
    assert!(session.pending_encoder_directive().is_none());

    session.queue_encoder_directive(EncoderDirectiveKind::AbrDownshift, 720);
    assert!(session.pending_encoder_directive().is_some());
    session.disconnect();
    assert!(session.pending_encoder_directive().is_none());
}

#[test]
fn matching_first_idr_commits_generation_and_enters_packetization() {
    let mut session = SenderSession::new(MemoryTransport::new());
    session
        .connect(Endpoint {
            host: "127.0.0.1".into(),
            port: 4433,
        })
        .expect("connect");
    session.force_status_for_test(SenderStatus::Streaming);
    session.set_stream_config(StreamConfigParams {
        width: 1920,
        height: 1080,
        ..Default::default()
    });
    assert!(session.report_encoder_started(0, 10, INITIAL_STREAM_EPOCH, 1080));

    let candidate_epoch = session.begin_stream_reconfiguration(720);
    let transaction_id = session.encoder_transaction_id_for_epoch(candidate_epoch);
    assert_ne!(transaction_id, 0);
    session.set_stream_config(StreamConfigParams {
        width: 1280,
        height: 720,
        ..Default::default()
    });
    assert!(session.report_encoder_started(transaction_id, 11, candidate_epoch, 720));
    assert!(matches!(
        session.ingest_encoder_access_unit(super::native_au(
            b"delta",
            false,
            1,
            (transaction_id, 11, candidate_epoch, 720)
        )),
        Err(SenderError::EncoderRefreshPending)
    ));
    assert!(matches!(
        session.ingest_encoder_access_unit(super::native_au(
            b"stale-idr",
            true,
            2,
            (transaction_id, 12, candidate_epoch, 720)
        )),
        Err(SenderError::EncoderRefreshPending)
    ));
    let packets = session
        .ingest_encoder_access_unit(super::native_au(
            b"first-matching-idr",
            true,
            3,
            (transaction_id, 11, candidate_epoch, 720),
        ))
        .expect("the commit IDR must also enter packetization");
    assert!(packets > 0);
    assert_eq!(session.current_stream_epoch(), candidate_epoch);
    assert_eq!(session.bitrate_active_height(), 720);
    assert!(!session.take_keyframe_request());
}

#[test]
fn rejected_commit_idr_does_not_commit_encoder_transaction() {
    let mut session = SenderSession::new(MemoryTransport::new());
    session
        .connect(Endpoint {
            host: "127.0.0.1".into(),
            port: 4433,
        })
        .expect("connect");
    session.force_status_for_test(SenderStatus::Streaming);
    session.set_stream_config(StreamConfigParams {
        width: 1920,
        height: 1080,
        ..Default::default()
    });
    assert!(session.report_encoder_started(0, 10, INITIAL_STREAM_EPOCH, 1080));

    let candidate_epoch = session.begin_stream_reconfiguration(720);
    let transaction_id = session.encoder_transaction_id_for_epoch(candidate_epoch);
    session.set_stream_config(StreamConfigParams {
        width: 1280,
        height: 720,
        ..Default::default()
    });
    assert!(session.report_encoder_started(transaction_id, 11, candidate_epoch, 720));

    let oversized = vec![
        0;
        picoo_protocol::MAX_FEC_FRAGMENT_PAYLOAD
            * usize::from(picoo_protocol::MAX_VIDEO_FRAGMENTS_PER_ACCESS_UNIT)
            + 1
    ];
    assert!(matches!(
        session.ingest_encoder_access_unit(super::native_au(
            &oversized,
            true,
            3,
            (transaction_id, 11, candidate_epoch, 720),
        )),
        Err(SenderError::AccessUnitTooLarge)
    ));
    assert_eq!(session.current_stream_epoch(), INITIAL_STREAM_EPOCH);
    assert_eq!(
        session.encoder_transaction_id_for_epoch(candidate_epoch),
        transaction_id
    );
    assert_eq!(session.pending_packets(), 0);

    session
        .ingest_encoder_access_unit(super::native_au(
            b"valid-commit-idr",
            true,
            4,
            (transaction_id, 11, candidate_epoch, 720),
        ))
        .expect("a later valid IDR commits the still-pending transaction");
    assert_eq!(session.current_stream_epoch(), candidate_epoch);
}

#[test]
fn encoder_failure_policy_is_owned_by_rust() {
    let mut session = SenderSession::new(MemoryTransport::new());
    session.set_stream_config(StreamConfigParams {
        width: 1920,
        height: 1080,
        ..Default::default()
    });
    assert!(session.report_encoder_started(0, 20, INITIAL_STREAM_EPOCH, 1080));

    let untouched_epoch = session.begin_stream_reconfiguration(720);
    let untouched_id = session.encoder_transaction_id_for_epoch(untouched_epoch);
    assert_eq!(
        session.report_encoder_failed(untouched_id, 0),
        EncoderFailureOutcome::RolledBack
    );
    assert!(session.pending_encoder_directive().is_none());
    assert_eq!(session.current_stream_epoch(), INITIAL_STREAM_EPOCH);

    let failed_epoch = session.begin_stream_reconfiguration(720);
    let failed_id = session.encoder_transaction_id_for_epoch(failed_epoch);
    assert!(session.report_encoder_started(failed_id, 21, failed_epoch, 720));
    assert_eq!(
        session.report_encoder_failed(failed_id, 21),
        EncoderFailureOutcome::RecoveryRequested
    );
    let recovery = session
        .pending_encoder_directive()
        .expect("recovery effect");
    assert_eq!(recovery.kind, EncoderDirectiveKind::Recovery);
    assert_eq!(recovery.stream_epoch, INITIAL_STREAM_EPOCH);
    assert_eq!(recovery.target_height, 1080);

    session.set_stream_config(StreamConfigParams {
        width: 1920,
        height: 1080,
        ..Default::default()
    });
    assert!(session.report_encoder_started(
        recovery.id,
        22,
        recovery.stream_epoch,
        recovery.target_height,
    ));
    session
        .connect(Endpoint {
            host: "127.0.0.1".into(),
            port: 4433,
        })
        .expect("connect");
    session.force_status_for_test(SenderStatus::Streaming);
    session
        .ingest_encoder_access_unit(super::native_au(
            b"recovery-idr",
            true,
            4,
            (
                recovery.id,
                22,
                recovery.stream_epoch,
                recovery.target_height,
            ),
        ))
        .expect("matching recovery IDR");
    assert_eq!(session.current_stream_epoch(), INITIAL_STREAM_EPOCH);
    assert!(session.pending_encoder_directive().is_none());
}

#[test]
fn committed_encoder_runtime_failure_requests_rust_owned_recovery() {
    let mut session = SenderSession::new(MemoryTransport::new());
    session.set_stream_config(StreamConfigParams {
        width: 1280,
        height: 720,
        ..Default::default()
    });
    assert!(session.report_encoder_started(0, 20, INITIAL_STREAM_EPOCH, 720));

    assert_eq!(
        session.report_encoder_failed(0, 20),
        EncoderFailureOutcome::RecoveryRequested
    );
    let recovery = session
        .pending_encoder_directive()
        .expect("committed failure creates recovery effect");
    assert_eq!(recovery.kind, EncoderDirectiveKind::Recovery);
    assert_eq!(recovery.stream_epoch, INITIAL_STREAM_EPOCH);
    assert_eq!(recovery.target_height, 720);

    assert_eq!(
        session.report_encoder_failed(0, 20),
        EncoderFailureOutcome::Ignored,
        "stale committed-generation errors cannot replace an active recovery"
    );
}

#[test]
fn recovery_failure_disconnects_instead_of_recursing() {
    let mut session = SenderSession::new(MemoryTransport::new());
    session
        .connect(Endpoint {
            host: "127.0.0.1".into(),
            port: 4433,
        })
        .expect("connect");
    session.set_stream_config(StreamConfigParams {
        width: 1920,
        height: 1080,
        ..Default::default()
    });
    assert!(session.report_encoder_started(0, 30, INITIAL_STREAM_EPOCH, 1080));
    let failed_epoch = session.begin_stream_reconfiguration(720);
    let failed_id = session.encoder_transaction_id_for_epoch(failed_epoch);
    assert!(session.report_encoder_started(failed_id, 31, failed_epoch, 720));
    assert_eq!(
        session.report_encoder_failed(failed_id, 31),
        EncoderFailureOutcome::RecoveryRequested
    );
    let recovery = session.pending_encoder_directive().expect("recovery");
    assert_eq!(
        session.report_encoder_failed(recovery.id, 0),
        EncoderFailureOutcome::Disconnected
    );
    assert_eq!(session.status(), SenderStatus::Disconnected);
    assert_eq!(
        session.last_session_error(),
        Some("ENCODER_RECOVERY_FAILED")
    );
}
