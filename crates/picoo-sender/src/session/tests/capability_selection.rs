use super::*;

#[test]
fn receiver_capability_does_not_replace_explicit_source_preference() {
    let mut session = SenderSession::new(MemoryTransport::new());
    session.set_stream_config(super::source_configuration(720));
    let capabilities = decoder_capabilities(&[(1280, 720)]);
    assert!(session.apply_capabilities_for_test(capabilities));
    session.set_preferred_height(1080);
    assert_eq!(session.receiver_max_height(), 720);
    assert_eq!(session.bitrate.preferred_height(), 1080);

    let expanded = decoder_capabilities(&[(1920, 1080)]);
    assert!(session.apply_capabilities_for_test(expanded));
    assert_eq!(session.bitrate.preferred_height(), 1080);
}

fn decoder_capabilities(sizes: &[(u32, u32)]) -> Capabilities {
    use picoo_protocol::control::{ColorRange, DecoderOffer, FrameRate, VideoCodec, VideoFormat};
    Capabilities {
        offers: sizes
            .iter()
            .map(|&(width, height)| DecoderOffer {
                format: Some(VideoFormat::sdr_709(
                    VideoCodec::Avc,
                    Resolution { width, height },
                    FrameRate {
                        numerator: 30,
                        denominator: 1,
                    },
                    ColorRange::Limited,
                )),
                max_access_unit_bytes: picoo_protocol::MAX_MEDIA_ACCESS_UNIT_BYTES,
                max_level_idc: if height == 720 { 31 } else { 40 },
            })
            .collect(),
    }
}

#[test]
fn sender_does_not_borrow_height_from_another_codec_or_frame_rate() {
    use picoo_protocol::control::{VideoCodec, VideoProfile};
    let mut session = SenderSession::new(MemoryTransport::new());
    session.set_stream_config(super::source_configuration(720));
    let mut caps = decoder_capabilities(&[(1280, 720), (1920, 1080)]);
    let hevc = &mut caps.offers[1];
    hevc.max_level_idc = 123;
    let format = hevc.format.as_mut().unwrap();
    format.codec = VideoCodec::Hevc as i32;
    format.profile = VideoProfile::HevcMain as i32;
    assert!(session.apply_capabilities_for_test(caps.clone()));
    assert_eq!(session.receiver_max_height(), 720);

    caps.offers[1] = decoder_capabilities(&[(1920, 1080)]).offers.remove(0);
    caps.offers[1].max_level_idc = 42;
    caps.offers[1]
        .format
        .as_mut()
        .unwrap()
        .frame_rate
        .as_mut()
        .unwrap()
        .numerator = 60;
    assert!(session.apply_capabilities_for_test(caps.clone()));
    assert_eq!(session.receiver_max_height(), 720);

    caps.offers.remove(0);
    assert!(!session.apply_capabilities_for_test(caps));
    assert_eq!(
        session.last_session_error.as_deref(),
        Some("NO_MATCHING_DECODER_OFFER")
    );
}

#[test]
fn capabilities_before_source_selection_do_not_invent_avc_or_thirty_fps() {
    use picoo_protocol::control::{VideoCodec, VideoProfile};
    let mut session = SenderSession::new(MemoryTransport::new());
    let mut caps = decoder_capabilities(&[(1920, 1080)]);
    caps.offers[0].max_level_idc = 123;
    let format = caps.offers[0].format.as_mut().unwrap();
    format.codec = VideoCodec::Hevc as i32;
    format.profile = VideoProfile::HevcMain as i32;
    format.frame_rate.as_mut().unwrap().numerator = 60;
    assert!(session.apply_capabilities_for_test(caps));
    assert_eq!(session.receiver_max_height(), 0);
    let previous_epoch = session.last_allocated_stream_epoch;
    let previous_id = session.next_encoder_directive_id;
    assert_eq!(
        session.begin_stream_reconfiguration(crate::SourceFormat {
            codec: picoo_bitstream::Codec::Avc,
            height: 1080,
            fps: 30,
        }),
        0
    );
    assert_eq!(session.last_allocated_stream_epoch, previous_epoch);
    assert_eq!(session.next_encoder_directive_id, previous_id);
    assert!(
        session.begin_stream_reconfiguration(crate::SourceFormat {
            codec: picoo_bitstream::Codec::Hevc,
            height: 1080,
            fps: 60,
        }) > previous_epoch
    );
    assert_eq!(session.receiver_max_height(), 1080);
    assert_ne!(
        session.last_session_error.as_deref(),
        Some("NO_MATCHING_DECODER_OFFER")
    );
}

#[test]
fn explicit_request_cannot_combine_dimensions_rate_or_color_from_other_offers() {
    let mut session = SenderSession::new(MemoryTransport::new());
    let mut caps = decoder_capabilities(&[(1280, 720), (1920, 1080)]);
    caps.offers[1].max_level_idc = 42;
    caps.offers[1]
        .format
        .as_mut()
        .unwrap()
        .frame_rate
        .as_mut()
        .unwrap()
        .numerator = 60;
    assert!(session.apply_capabilities_for_test(caps));
    for (height, fps) in [(720, 60), (1080, 30)] {
        assert_eq!(
            session.begin_stream_reconfiguration(crate::SourceFormat {
                codec: picoo_bitstream::Codec::Avc,
                height,
                fps,
            }),
            0
        );
    }
    let mut full_range = decoder_capabilities(&[(1280, 720)]);
    full_range.offers[0]
        .format
        .as_mut()
        .unwrap()
        .color
        .as_mut()
        .unwrap()
        .range = picoo_protocol::control::ColorRange::Full as i32;
    assert!(session.apply_capabilities_for_test(full_range));
    assert_eq!(
        session.begin_stream_reconfiguration(crate::SourceFormat {
            codec: picoo_bitstream::Codec::Avc,
            height: 720,
            fps: 30,
        }),
        0
    );
    assert_eq!(session.last_allocated_stream_epoch, INITIAL_STREAM_EPOCH);
}

#[test]
fn preparation_matches_visible_size_without_guessing_native_storage_padding() {
    let mut session = SenderSession::new(MemoryTransport::new());
    let mut caps = decoder_capabilities(&[(1920, 1080)]);
    caps.offers[0]
        .format
        .as_mut()
        .unwrap()
        .coded_size
        .as_mut()
        .unwrap()
        .height = 1088;
    assert!(session.apply_capabilities_for_test(caps));
    assert!(
        session.begin_stream_reconfiguration(crate::SourceFormat {
            codec: picoo_bitstream::Codec::Avc,
            height: 1080,
            fps: 30,
        }) > INITIAL_STREAM_EPOCH
    );
    assert_eq!(session.receiver_max_height(), 1080);
}
