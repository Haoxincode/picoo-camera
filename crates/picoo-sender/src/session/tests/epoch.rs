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
    let committed_epoch = session.current_stream_epoch();
    let pending_epoch = session.begin_stream_reconfiguration();
    assert_ne!(pending_epoch, committed_epoch);
    session
        .ingest_access_unit(b"still-current", true, 1, committed_epoch)
        .expect("committed epoch remains valid while apply is pending");
    assert!(matches!(
        session.ingest_access_unit(b"not-committed", true, 2, pending_epoch),
        Err(SenderError::StaleStreamEpoch { got, current })
            if got == pending_epoch && current == committed_epoch
    ));
    assert!(session.report_encoder_height(720, pending_epoch));
    assert_eq!(session.current_stream_epoch(), pending_epoch);
    assert!(matches!(
        session.ingest_access_unit(b"now-stale", true, 3, committed_epoch),
        Err(SenderError::StaleStreamEpoch { got, current })
            if got == committed_epoch && current == pending_epoch
    ));
    assert!(matches!(
        session.ingest_access_unit(b"before-config", true, 4, pending_epoch),
        Err(SenderError::StreamConfigPending { stream_epoch })
            if stream_epoch == pending_epoch
    ));
    session.set_stream_config(StreamConfigParams {
        width: 1280,
        height: 720,
        ..Default::default()
    });
    session
        .send_pending_stream_config()
        .expect("queue matching config before media");
    session
        .ingest_access_unit(b"current", true, 5, pending_epoch)
        .expect("committed pending epoch accepted");
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
    session.stream_config_sent = true;
    let committed = session.current_stream_epoch();
    let pending = session.begin_stream_reconfiguration();
    assert_ne!(pending, committed);
    assert!(session.stream_config_sent());
    assert_eq!(
        session
            .pending_stream_config()
            .map(|config| config.stream_epoch),
        Some(committed)
    );

    assert!(session.report_encoder_height(720, pending));
    assert!(!session.stream_config_sent());
    assert!(session.pending_stream_config().is_none());
    assert!(session.media_blocked_for_stream_config);

    session.set_stream_config(StreamConfigParams {
        width: 1280,
        height: 720,
        ..Default::default()
    });
    session
        .send_pending_stream_config()
        .expect("new config is sent");
    assert_eq!(
        session
            .pending_stream_config()
            .map(|config| config.stream_epoch),
        Some(pending)
    );
    assert!(!session.media_blocked_for_stream_config);
}

#[test]
fn current_epoch_report_is_idempotent_not_a_resolution_transition() {
    let mut session = SenderSession::new(MemoryTransport::new());
    let epoch = session.current_stream_epoch();
    session.set_stream_config(StreamConfigParams {
        width: 1920,
        height: 1080,
        ..Default::default()
    });
    assert!(session.report_encoder_height(1080, epoch));
    assert!(session.report_encoder_height(1080, epoch));
    assert!(!session.report_encoder_height(720, epoch));
    assert_eq!(session.bitrate_active_height(), 1080);
}

#[test]
fn stream_epoch_exhausts_before_crossing_android_signed_range() {
    let mut session = SenderSession::new(MemoryTransport::new());
    session.last_allocated_stream_epoch = MAX_STREAM_EPOCH;
    assert_eq!(session.begin_stream_reconfiguration(), 0);
    assert_eq!(session.current_stream_epoch(), INITIAL_STREAM_EPOCH);
    assert_eq!(session.last_session_error(), Some("STREAM_EPOCH_EXHAUSTED"));
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
    let pending = session.begin_stream_reconfiguration();
    session.set_stream_config(StreamConfigParams {
        width: 1280,
        height: 720,
        sps: vec![1, 2, 3],
        ..Default::default()
    });
    assert!(session.report_encoder_height(720, pending));
    let config = session.pending_stream_config().expect("staged config");
    assert_eq!(config.stream_epoch, pending);
    assert_eq!(config.sps, vec![1, 2, 3]);
    assert!(session.media_blocked_for_stream_config);
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
    let pending = session.begin_stream_reconfiguration();
    assert!(session.report_encoder_height(720, pending));
    session.set_stream_config(StreamConfigParams {
        width: 1920,
        height: 1080,
        ..Default::default()
    });
    assert!(matches!(
        session.send_pending_stream_config(),
        Err(SenderError::StreamConfigHeightMismatch {
            expected: 720,
            got: 1080
        })
    ));
    assert!(matches!(
        session.ingest_access_unit(b"blocked", true, 1, pending),
        Err(SenderError::StreamConfigPending { .. })
    ));
}

#[test]
fn noncanonical_encoder_height_cannot_commit_ladder_epoch() {
    let mut session = SenderSession::new(MemoryTransport::new());
    let pending = session.begin_stream_reconfiguration();
    session.set_stream_config(StreamConfigParams {
        width: 1280,
        height: 800,
        ..Default::default()
    });
    assert!(!session.report_encoder_height(800, pending));
    assert_eq!(session.current_stream_epoch(), INITIAL_STREAM_EPOCH);
}

#[test]
fn cancelled_reconfiguration_restores_committed_stream_config() {
    let mut session = SenderSession::new(MemoryTransport::new());
    session.stream_config_sent = true;
    let committed = session.pending_stream_config().cloned();
    let pending = session.begin_stream_reconfiguration();
    session.set_stream_config(StreamConfigParams {
        width: 854,
        height: 480,
        ..Default::default()
    });
    assert_eq!(session.pending_stream_config().map(|c| c.height), Some(480));
    assert!(session.cancel_stream_reconfiguration(pending));
    assert_eq!(session.pending_stream_config().cloned(), committed);
    assert!(session.stream_config_sent());
}

#[test]
fn disconnect_aborts_pending_local_and_directive_generations() {
    let mut session = SenderSession::new(MemoryTransport::new());
    let local = session.begin_stream_reconfiguration();
    assert_eq!(session.pending_local_stream_epoch, Some(local));
    session.disconnect();
    assert_eq!(session.pending_local_stream_epoch, None);

    session.queue_encoder_directive(EncoderDirectiveKind::AbrDownshift, 720);
    assert!(session.pending_encoder_directive().is_some());
    session.disconnect();
    assert!(session.pending_encoder_directive().is_none());
}
