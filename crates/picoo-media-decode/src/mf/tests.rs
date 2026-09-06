use super::*;

#[test]
fn pack_frame_size_matches_mf_convention() {
    assert_eq!(pack_frame_size(1280, 720), (1280u64 << 32) | 720);
}

#[test]
fn sequence_header_from_config_builds_annex_b() {
    let (sps, pps) =
        picoo_bitstream::avc::extract_sps_pps(picoo_testkit::H264_1280X720_RED_IDR).unwrap();
    let cfg = StreamConfig {
        codec: picoo_protocol::control::VideoCodec::Avc as i32,
        codec_configuration: picoo_bitstream::CodecConfiguration::from_avc_parameter_sets(
            &sps, &pps,
        )
        .unwrap()
        .record()
        .to_vec(),
        width: 1280,
        height: 720,
        ..Default::default()
    };
    let header = MfVideoDecoder::sequence_header_from_config(Some(&cfg)).unwrap();
    assert_eq!(
        header,
        picoo_bitstream::avc::annex_b_parameter_sets(&sps, &pps)
    );
}

#[test]
fn native_coded_allocation_preserves_visible_geometry_and_rejects_conflicting_config() {
    // REQ-PICOO-MEDIA-032: the real fixture has coded 192x96, visible 64x64.
    let annex = picoo_testkit::AVC_64X64_BT709_IDR;
    let (sps, pps) = picoo_bitstream::avc::extract_sps_pps(annex).unwrap();
    let wire = picoo_bitstream::canonical_access_unit(
        picoo_bitstream::Codec::Avc,
        picoo_bitstream::NalFormat::AnnexB,
        annex,
    )
    .unwrap();
    let mut config = StreamConfig {
        codec: picoo_protocol::control::VideoCodec::Avc as i32,
        width: 64,
        height: 64,
        fps: 30,
        codec_configuration: picoo_bitstream::CodecConfiguration::from_avc_parameter_sets(
            &sps, &pps,
        )
        .unwrap()
        .record()
        .to_vec(),
        ..Default::default()
    };
    let mut decoder = MfVideoDecoder::software_diagnostic().unwrap();
    let submitted = decoder.decode_fixture(&wire, Some(&config)).unwrap();
    let frame = if let Some(frame) = submitted.frames.into_iter().next() {
        frame.frame
    } else {
        // A synchronous MFT can retain its last picture until EOS. Drain
        // the accepted input; never submit the same AU again to force output.
        unsafe {
            use windows::Win32::Media::MediaFoundation::{
                MFT_MESSAGE_COMMAND_DRAIN, MFT_MESSAGE_NOTIFY_END_OF_STREAM,
            };
            decoder
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0)
                .unwrap();
            decoder
                .transform
                .ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0)
                .unwrap();
            decoder
                .drain_frames()
                .unwrap()
                .into_iter()
                .next()
                .expect("EOS drain releases the accepted picture")
                .frame
        }
    };
    assert_eq!(
        (frame.description().width, frame.description().height),
        (64, 64)
    );
    assert!(frame.native_image().windows().is_some());
    assert_eq!(frame.description().native_format.visible_rect.width, 64);
    let geometry = decoder.geometry;
    let header = decoder.sequence_header.clone();
    config.width = 1280;
    assert!(matches!(
        decoder.decode_fixture(&wire, Some(&config)),
        Err(DecodeError::ConfigurationMismatch)
    ));
    assert_eq!(decoder.geometry, geometry);
    assert_eq!(decoder.sequence_header, header);
}

#[test]
fn existing_sta_apartment_is_borrowed_not_replaced() {
    assert!(!com_initialization_ownership(RPC_E_CHANGED_MODE).expect("borrow STA"));
    assert!(com_initialization_ownership(HRESULT(0)).expect("own successful init"));
}

#[test]
fn decoder_starts_inside_existing_sta_apartment() {
    let initialized =
        unsafe { CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED) };
    initialized.ok().expect("initialize fixture STA");
    let decoder =
        MfVideoDecoder::software_diagnostic().expect("create diagnostic MF decoder inside STA");
    drop(decoder);
    unsafe { CoUninitialize() };
}

#[test]
fn hevc_configuration_is_parsed_before_native_codec_admission() {
    let mut decoder = MfVideoDecoder::software_diagnostic().unwrap();
    let config = StreamConfig {
        codec: picoo_protocol::control::VideoCodec::Hevc as i32,
        width: 1280,
        height: 720,
        fps: 30,
        codec_configuration: include_bytes!(
            "../../../picoo-testkit/fixtures/hevc-64x64-bt709-config.bin"
        )
        .to_vec(),
        ..Default::default()
    };
    let transform = decoder.transform.clone();
    assert!(matches!(
        decoder.decode_fixture(
            include_bytes!("../../../picoo-testkit/fixtures/hevc-64x64-bt709-idr.bin"),
            Some(&config)
        ),
        Err(DecodeError::ConfigurationMismatch)
    ));
    assert_eq!(transform, decoder.transform);
    assert_eq!(decoder.codec, Codec::Avc);
}

// Windows runners may lack the optional system HEVC codec. Run explicitly on
// hosts with the component installed; absence is failure, never fake decode success.
#[test]
#[ignore = "requires installed Windows system HEVC decoder"]
fn system_hevc_decodes_idr_and_switches_back_to_avc() {
    let mut decoder = MfVideoDecoder::software_diagnostic().unwrap();
    let config = StreamConfig {
        codec: picoo_protocol::control::VideoCodec::Hevc as i32,
        width: 64,
        height: 64,
        fps: 30,
        codec_configuration: include_bytes!(
            "../../../picoo-testkit/fixtures/hevc-64x64-bt709-config.bin"
        )
        .to_vec(),
        ..Default::default()
    };
    let result = decoder
        .decode_fixture(
            include_bytes!("../../../picoo-testkit/fixtures/hevc-64x64-bt709-idr.bin"),
            Some(&config),
        )
        .unwrap();
    assert!(result.refresh_accepted);
    let mut frames = result.frames;
    unsafe {
        use windows::Win32::Media::MediaFoundation::{
            MFT_MESSAGE_COMMAND_DRAIN, MFT_MESSAGE_NOTIFY_END_OF_STREAM,
        };
        decoder
            .transform
            .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0)
            .unwrap();
        decoder
            .transform
            .ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0)
            .unwrap();
    }
    frames.extend(decoder.drain_frames().unwrap());
    assert_eq!(frames.len(), 1);
    assert_eq!(
        (
            frames[0].frame.description().width,
            frames[0].frame.description().height
        ),
        (64, 64)
    );
    assert_eq!(
        frames[0].token.stream_config.as_ref().unwrap().codec,
        config.codec
    );
    let avc = picoo_bitstream::canonical_access_unit(
        Codec::Avc,
        picoo_bitstream::NalFormat::AnnexB,
        picoo_testkit::AVC_64X64_BT709_IDR,
    )
    .unwrap();
    decoder.decode_fixture(&avc, None).unwrap();
    assert_eq!(decoder.codec, Codec::Avc);
}
