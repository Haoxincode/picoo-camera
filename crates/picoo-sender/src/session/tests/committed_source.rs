//! REQ-PICOO-MEDIA-055: requested and committed source formats are distinct.
use super::*;

fn submit(
    session: &mut SenderSession<MemoryTransport>,
    config: StreamConfigParams,
    generation: u64,
    epoch: u32,
) -> Result<crate::EncoderEventOutcome, SenderError> {
    session.submit_encoder_event(crate::NativeEncoderEvent {
        data: b"native-idr",
        is_keyframe: true,
        pts_us: 1,
        encoded_at_us: 2,
        encoder_generation: generation,
        stream_epoch: epoch,
        width: config.width,
        height: config.height,
        stream_config: Some(config),
    })
}

#[test]
fn same_generation_cannot_change_source_even_when_both_formats_are_offered() {
    let initial = source_configuration(720);
    let mut hevc = initial.clone();
    hevc.configuration = picoo_bitstream::CodecConfiguration::parse(
        picoo_bitstream::Codec::Hevc,
        Bytes::from_static(include_bytes!(
            "../../../../picoo-media-decode/probes/apple-native-formats/2-720-60.config"
        )),
    )
    .unwrap()
    .into();
    for initial in [initial, hevc.clone()] {
        let mut changed = hevc.clone();
        changed.fps = if initial.configuration.codec() == picoo_bitstream::Codec::Hevc {
            60
        } else {
            30
        };
        let mut session = SenderSession::new(MemoryTransport::new());
        assert_eq!(session.committed_source_format(), None);
        session
            .connect(Endpoint {
                host: "127.0.0.1".into(),
                port: 4433,
            })
            .unwrap();
        session.force_status_for_test(SenderStatus::Streaming);
        let mut caps = exact_capabilities(&initial);
        caps.offers.extend(exact_capabilities(&changed).offers);
        assert!(session.apply_capabilities_for_test(caps));
        let epoch = session.current_stream_epoch();
        assert!(
            submit(&mut session, initial.clone(), 10, epoch)
                .unwrap()
                .encoder_accepted
        );
        assert_eq!(
            session.committed_source_format(),
            Some(initial.source_format())
        );
        let before = session.pending_stream_config.clone();
        let control = session.next_control_message_id;
        let sent = session.transport.sent_order.len();
        assert!(submit(&mut session, changed.clone(), 10, epoch).is_err());
        assert_eq!(
            session.committed_source_format(),
            Some(initial.source_format())
        );
        assert_eq!(session.pending_stream_config, before);
        assert_eq!(session.next_control_message_id, control);
        assert_eq!(session.transport.sent_order.len(), sent);
        assert_eq!(session.pending_packets(), 0);

        // Bitrate and presentation metadata do not change the source request.
        let mut metadata = initial.clone();
        metadata.bitrate_bps += 1000;
        metadata.mirrored = !metadata.mirrored;
        assert!(
            submit(&mut session, metadata, 10, epoch)
                .unwrap()
                .encoder_accepted
        );
        assert_eq!(
            session.committed_source_format(),
            Some(initial.source_format())
        );

        let failed_candidate = session.begin_stream_reconfiguration(changed.source_format());
        assert_ne!(failed_candidate, 0);
        let transaction = session.encoder_transaction_id_for_epoch(failed_candidate);
        // Failure before native start must roll back without inventing a format.
        assert_eq!(
            session.report_encoder_failed(transaction, 0),
            crate::EncoderFailureOutcome::RolledBack
        );
        assert_eq!(
            session.committed_source_format(),
            Some(initial.source_format())
        );
        let candidate = session.begin_stream_reconfiguration(changed.source_format());
        assert_ne!(candidate, 0);
        assert_eq!(
            session.committed_source_format(),
            Some(initial.source_format())
        );
        assert!(
            submit(&mut session, changed.clone(), 11, candidate)
                .unwrap()
                .encoder_accepted
        );
        assert_eq!(
            session.committed_source_format(),
            Some(changed.source_format())
        );
        session.disconnect();
        assert_eq!(
            session.committed_source_format(),
            Some(changed.source_format())
        );
    }
}
