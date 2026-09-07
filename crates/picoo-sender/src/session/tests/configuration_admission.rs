use super::*;

#[test]
fn actual_offer_rejection_preserves_generation_configuration_and_control_identity() {
    use picoo_protocol::control::{ColorRange, DecoderOffer};
    let mut source = super::source_configuration(720);
    source.fps = 30;
    source.configuration = picoo_bitstream::CodecConfiguration::parse(
        picoo_bitstream::Codec::Hevc,
        Bytes::from_static(include_bytes!(
            "../../../../picoo-media-decode/probes/xiaomi-native-formats/2-720-30.config"
        )),
    )
    .unwrap()
    .into();
    let wire = source.to_proto().unwrap();
    let exact = DecoderOffer {
        format: Some(wire.validated_video_format().unwrap()),
        max_level_idc: wire.level_idc,
        max_access_unit_bytes: 1024,
    };
    let mut wrong_storage = exact;
    wrong_storage
        .format
        .as_mut()
        .unwrap()
        .coded_size
        .as_mut()
        .unwrap()
        .height = 720;
    let mut wrong_color = exact;
    wrong_color
        .format
        .as_mut()
        .unwrap()
        .color
        .as_mut()
        .unwrap()
        .range = ColorRange::Full as i32;
    let mut too_small = exact;
    too_small.max_access_unit_bytes = 4;
    let mut wrong_tier = exact;
    wrong_tier.format.as_mut().unwrap().tier = picoo_protocol::control::VideoTier::HevcMain as i32;
    assert_ne!(wrong_tier.format, exact.format);
    for offers in [
        vec![wrong_storage],
        vec![wrong_storage, wrong_color],
        vec![too_small],
        vec![wrong_tier],
    ] {
        let mut session = SenderSession::new(MemoryTransport::new());
        session
            .connect(Endpoint {
                host: "127.0.0.1".into(),
                port: 4433,
            })
            .unwrap();
        session.force_status_for_test(SenderStatus::Streaming);
        let caps = Capabilities { offers };
        caps.validate().unwrap();
        assert!(session.apply_capabilities_for_test(caps));
        let epoch = session.current_stream_epoch();
        let control = session.next_control_message_id;
        let attempt = |session: &mut SenderSession<MemoryTransport>| {
            session.submit_encoder_event(crate::NativeEncoderEvent {
                data: b"native-idr",
                is_keyframe: true,
                pts_us: 1,
                encoded_at_us: 2,
                encoder_generation: 10,
                stream_epoch: epoch,
                width: 1280,
                height: 720,
                stream_config: Some(source.clone()),
            })
        };
        assert!(attempt(&mut session).is_err());
        assert_eq!(session.current_stream_epoch(), epoch);
        assert_eq!(session.committed_encoder_generation, 0);
        assert!(session.pending_stream_config.is_none());
        assert_eq!(session.next_control_message_id, control);
        assert_eq!(session.pending_packets(), 0);
        // A rejected native fact does not poison a later exact offer.
        assert!(session.apply_capabilities_for_test(Capabilities {
            offers: vec![exact]
        }));
        assert!(attempt(&mut session).unwrap().encoder_accepted);
    }
}

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
    let candidate_epoch = session.begin_stream_reconfiguration(crate::SourceFormat {
        codec: picoo_bitstream::Codec::Avc,
        height: 1080,
        fps: 30,
    });
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
            Err(SenderError::Protocol(_))
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

#[test]
fn matching_height_and_generation_cannot_commit_another_codec_or_frame_rate() {
    let mut session = SenderSession::new(MemoryTransport::new());
    session
        .connect(Endpoint {
            host: "127.0.0.1".into(),
            port: 4433,
        })
        .unwrap();
    session.force_status_for_test(SenderStatus::Streaming);
    session.set_stream_config(super::source_configuration(720));
    let old_epoch = session.current_stream_epoch();
    assert!(session.report_encoder_started(0, 10, old_epoch, 720));
    let epoch = session.begin_stream_reconfiguration(crate::SourceFormat {
        codec: picoo_bitstream::Codec::Hevc,
        height: 720,
        fps: 60,
    });
    let transaction = session.encoder_transaction_id_for_epoch(epoch);
    assert!(session.report_encoder_started(transaction, 11, epoch, 720));
    let mut desired = super::source_configuration(720);
    desired.fps = 60;
    desired.configuration = picoo_bitstream::CodecConfiguration::parse(
        picoo_bitstream::Codec::Hevc,
        bytes::Bytes::from_static(include_bytes!(
            "../../../../picoo-media-decode/probes/apple-native-formats/2-720-60.config"
        )),
    )
    .unwrap()
    .into();
    let mut wrong_codec = desired.clone();
    wrong_codec.configuration = super::source_configuration(720).configuration;
    let mut wrong_fps = desired.clone();
    wrong_fps.fps = 30;
    for config in [wrong_codec, wrong_fps] {
        let before = session.pending_stream_config.clone();
        let control = session.next_control_message_id;
        assert!(session
            .submit_encoder_event(crate::NativeEncoderEvent {
                data: b"native-idr",
                is_keyframe: true,
                pts_us: 1,
                encoded_at_us: 2,
                encoder_generation: 11,
                stream_epoch: epoch,
                width: 1280,
                height: 720,
                stream_config: Some(config),
            })
            .is_err());
        assert_eq!(session.current_stream_epoch(), old_epoch);
        assert_eq!(session.pending_stream_config, before);
        assert_eq!(session.next_control_message_id, control);
        assert_eq!(session.pending_packets(), 0);
    }
    assert!(session.apply_capabilities_for_test(super::exact_capabilities(&desired)));
    let outcome = session
        .submit_encoder_event(crate::NativeEncoderEvent {
            data: b"matching-native-idr",
            is_keyframe: true,
            pts_us: 3,
            encoded_at_us: 4,
            encoder_generation: 11,
            stream_epoch: epoch,
            width: 1280,
            height: 720,
            stream_config: Some(desired),
        })
        .unwrap();
    assert!(outcome.encoder_accepted && outcome.stream_configured);
    assert_eq!(session.current_stream_epoch(), epoch);
}

#[test]
fn encoder_event_before_capabilities_cannot_bind_or_send_media() {
    let mut session = SenderSession::new(MemoryTransport::new());
    session
        .connect(Endpoint {
            host: "127.0.0.1".into(),
            port: 4433,
        })
        .unwrap();
    session.force_status_for_test(SenderStatus::Streaming);
    let control = session.next_control_message_id;
    let outcome = session.submit_encoder_event(crate::NativeEncoderEvent {
        data: b"early-native-idr",
        is_keyframe: true,
        pts_us: 1,
        encoded_at_us: 2,
        encoder_generation: 10,
        stream_epoch: session.current_stream_epoch(),
        width: 1280,
        height: 720,
        stream_config: Some(super::source_configuration(720)),
    });
    let outcome = outcome.expect("waiting for capability evidence is a normal rejection");
    assert!(!outcome.encoder_accepted);
    assert!(!outcome.stream_configured);
    assert_eq!(session.committed_encoder_generation, 0);
    assert!(session.pending_stream_config.is_none());
    assert_eq!(session.next_control_message_id, control);
    assert_eq!(session.pending_packets(), 0);
}
