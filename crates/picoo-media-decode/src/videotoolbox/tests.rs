use super::*;
use picoo_bitstream::avc::{extract_sps_pps, split_annex_b_nals};

use picoo_testkit::{
    AVC_1280X720_BT709_IDR as H264_1280X720_RED_IDR, AVC_64X64_BT709_IDR as H264_64X64_RED_IDR,
};

fn wire(annex: &[u8]) -> Vec<u8> {
    picoo_bitstream::canonical_access_unit(
        picoo_bitstream::Codec::Avc,
        picoo_bitstream::NalFormat::AnnexB,
        annex,
    )
    .unwrap()
    .into_owned()
}

fn assert_native_red(frame: &DecodedFrame) {
    use objc2_core_video::*;
    // Explicit diagnostic read of one YUV sample, not a Decoder pixel API.
    unsafe {
        let buffer = frame.native_image().apple().unwrap().pixel_buffer();
        assert!(CVPixelBufferGetIOSurface(Some(buffer)).is_some());
        assert_eq!(
            CVPixelBufferLockBaseAddress(buffer, CVPixelBufferLockFlags::ReadOnly),
            0
        );
        let y = *CVPixelBufferGetBaseAddressOfPlane(buffer, 0).cast::<u8>();
        let uv = CVPixelBufferGetBaseAddressOfPlane(buffer, 1).cast::<u8>();
        let chroma = [*uv, *uv.add(1)];
        assert_eq!(
            CVPixelBufferUnlockBaseAddress(buffer, CVPixelBufferLockFlags::ReadOnly),
            0
        );
        assert!(
            y > 16 && chroma[1] > chroma[0],
            "expected decoded red fixture"
        );
    }
}

#[test]
fn conflicting_in_band_configuration_does_not_replace_native_session() {
    let (sps, pps) = extract_sps_pps(H264_64X64_RED_IDR).unwrap();
    let config = StreamConfig {
        codec: picoo_protocol::control::VideoCodec::Avc as i32,
        width: 64,
        height: 64,
        codec_configuration: picoo_bitstream::CodecConfiguration::from_avc_parameter_sets(
            &sps, &pps,
        )
        .unwrap()
        .record()
        .to_vec(),
        ..Default::default()
    };
    let mut decoder = VideoToolboxDecoder::new();
    decoder
        .decode_fixture(&wire(H264_64X64_RED_IDR), Some(&config))
        .unwrap();
    let session = decoder.session.as_ref().map(CFRetained::as_ptr);
    assert!(matches!(
        decoder.decode_fixture(&wire(H264_1280X720_RED_IDR), Some(&config)),
        Err(DecodeError::ConfigurationMismatch)
    ));
    assert_eq!(session, decoder.session.as_ref().map(CFRetained::as_ptr));
    let frame = decoder
        .decode_fixture(&wire(H264_64X64_RED_IDR), Some(&config))
        .unwrap()
        .into_fixture_frame()
        .unwrap();
    assert_eq!(frame.description().width, 64);
}

#[test]
fn declared_geometry_mismatch_does_not_mutate_native_session() {
    let (sps, pps) = extract_sps_pps(H264_64X64_RED_IDR).unwrap();
    let mut config = StreamConfig {
        codec: picoo_protocol::control::VideoCodec::Avc as i32,
        width: 64,
        height: 64,
        codec_configuration: picoo_bitstream::CodecConfiguration::from_avc_parameter_sets(
            &sps, &pps,
        )
        .unwrap()
        .record()
        .to_vec(),
        ..Default::default()
    };
    let mut decoder = VideoToolboxDecoder::new();
    decoder
        .decode_fixture(&wire(H264_64X64_RED_IDR), Some(&config))
        .unwrap();
    let session = decoder.session.as_ref().map(CFRetained::as_ptr);
    config.width = 1280;
    assert!(matches!(
        decoder.decode_fixture(&wire(H264_64X64_RED_IDR), Some(&config)),
        Err(DecodeError::ConfigurationMismatch)
    ));
    assert_eq!(session, decoder.session.as_ref().map(CFRetained::as_ptr));
}

#[test]
fn unknown_native_color_is_rejected_instead_of_relabelled() {
    let mut decoder = VideoToolboxDecoder::new();
    assert!(decoder
        .decode_fixture(&wire(picoo_testkit::H264_64X64_RED_IDR), None)
        .is_err());
}

#[test]
fn videotoolbox_decodes_canonical_idr_to_native_frame() {
    let mut decoder = VideoToolboxDecoder::new();
    let frame = decoder
        .decode_fixture(&wire(H264_64X64_RED_IDR), None)
        .expect("VideoToolbox decode")
        .into_fixture_frame()
        .expect("decoded frame");
    assert_eq!(
        (frame.description().width, frame.description().height),
        (64, 64)
    );
    assert_native_red(&frame);
}

#[test]
fn videotoolbox_decodes_avcc_idr_with_stream_config() {
    let (sps, pps) = extract_sps_pps(H264_64X64_RED_IDR).expect("parameter sets");
    let idr = split_annex_b_nals(H264_64X64_RED_IDR)
        .into_iter()
        .find(|nal| nal.first().is_some_and(|byte| byte & 0x1f == 5))
        .expect("IDR");
    let mut avcc = Vec::with_capacity(idr.len() + 4);
    avcc.extend_from_slice(&(idr.len() as u32).to_be_bytes());
    avcc.extend_from_slice(idr);
    let config = StreamConfig {
        codec: picoo_protocol::control::VideoCodec::Avc as i32,
        width: 64,
        height: 64,
        codec_configuration: picoo_bitstream::CodecConfiguration::from_avc_parameter_sets(
            &sps, &pps,
        )
        .unwrap()
        .record()
        .to_vec(),
        ..Default::default()
    };

    let mut decoder = VideoToolboxDecoder::new();
    let frame = decoder
        .decode_fixture(&avcc, Some(&config))
        .expect("VideoToolbox decode")
        .into_fixture_frame()
        .expect("decoded frame");
    assert_eq!(
        (frame.description().width, frame.description().height),
        (64, 64)
    );
    assert_native_red(&frame);
}

#[test]
fn same_parameter_sets_reuse_session_and_reset_discards_it() {
    let (sps, pps) = extract_sps_pps(H264_64X64_RED_IDR).expect("parameter sets");
    let mut decoder = VideoToolboxDecoder::new();
    let configuration = CodecConfiguration::from_avc_parameter_sets(&sps, &pps).unwrap();
    decoder
        .ensure_session(&configuration)
        .expect("first session");
    let first = decoder.session.as_ref().map(CFRetained::as_ptr);
    decoder
        .ensure_session(&configuration)
        .expect("reused session");
    assert_eq!(first, decoder.session.as_ref().map(CFRetained::as_ptr));
    decoder.reset().expect("reset");
    assert!(decoder.session.is_none());
}

#[test]
fn malformed_access_unit_is_rejected_without_stub_fallback() {
    let mut decoder = VideoToolboxDecoder::new();
    let result = decoder.decode_fixture(b"not-h264", None);
    assert!(matches!(result, Err(DecodeError::UnsupportedAccessUnit)));
    assert!(decoder.session.is_none());
}

#[test]
fn in_band_parameter_change_recreates_session_and_updates_dimensions() {
    let mut decoder = VideoToolboxDecoder::new();
    let first = decoder
        .decode_fixture(&wire(H264_64X64_RED_IDR), None)
        .expect("64x64 decode")
        .into_fixture_frame()
        .expect("64x64 frame");
    assert_eq!(
        (first.description().width, first.description().height),
        (64, 64)
    );
    let first_sps = decoder.configuration.as_ref().unwrap().sps()[0].to_vec();

    let second = decoder
        .decode_fixture(&wire(H264_1280X720_RED_IDR), None)
        .expect("1280x720 decode")
        .into_fixture_frame()
        .expect("1280x720 frame");
    assert_eq!(
        (second.description().width, second.description().height),
        (1280, 720)
    );
    assert_native_red(&first);
    assert_ne!(
        first_sps,
        decoder.configuration.as_ref().unwrap().sps()[0].to_vec()
    );
    assert_eq!(
        decoder.configuration.as_ref().unwrap().sps()[0].to_vec(),
        extract_sps_pps(H264_1280X720_RED_IDR)
            .expect("1280x720 parameter sets")
            .0
    );
}

// REQ-PICOO-NEXT-003/011/025: real hardware HEVC and transactional codec replacement.
const HEVC_CONFIG: &[u8] =
    include_bytes!("../../../picoo-testkit/fixtures/hevc-64x64-bt709-config.bin");
const HEVC_IDR: &[u8] = include_bytes!("../../../picoo-testkit/fixtures/hevc-64x64-bt709-idr.bin");

fn hevc_config() -> StreamConfig {
    StreamConfig {
        codec: picoo_protocol::control::VideoCodec::Hevc as i32,
        width: 64,
        height: 64,
        codec_configuration: HEVC_CONFIG.to_vec(),
        ..Default::default()
    }
}

#[test]
fn hardware_hevc_decode_and_codec_switch_preserve_retained_images() {
    let mut decoder = VideoToolboxDecoder::new();
    let config = hevc_config();
    let hevc = decoder
        .decode_fixture(HEVC_IDR, Some(&config))
        .unwrap()
        .into_fixture_frame()
        .unwrap();
    assert_eq!(
        (hevc.description().width, hevc.description().height),
        (64, 64)
    );
    assert_native_red(&hevc);
    let first_session = decoder.session.as_ref().map(CFRetained::as_ptr);
    decoder.decode_fixture(HEVC_IDR, Some(&config)).unwrap();
    assert_eq!(
        first_session,
        decoder.session.as_ref().map(CFRetained::as_ptr)
    );
    let avc = decoder
        .decode_fixture(&wire(H264_64X64_RED_IDR), None)
        .unwrap()
        .into_fixture_frame()
        .unwrap();
    assert_eq!(decoder.configuration.as_ref().unwrap().codec(), Codec::Avc);
    assert_ne!(
        first_session,
        decoder.session.as_ref().map(CFRetained::as_ptr)
    );
    decoder.decode_fixture(HEVC_IDR, Some(&config)).unwrap();
    assert_eq!(decoder.configuration.as_ref().unwrap().codec(), Codec::Hevc);
    assert_native_red(&hevc);
    assert_native_red(&avc);
}

#[test]
fn conflicting_hevc_parameter_sets_preserve_working_session() {
    let mut decoder = VideoToolboxDecoder::new();
    let config = hevc_config();
    decoder.decode_fixture(HEVC_IDR, Some(&config)).unwrap();
    let session = decoder.session.as_ref().map(CFRetained::as_ptr);
    let conflicting = picoo_bitstream::canonical_access_unit(
        Codec::Hevc,
        picoo_bitstream::NalFormat::AnnexB,
        include_bytes!("../../../picoo-testkit/fixtures/hevc-1280x720-bt709-idr.h265"),
    )
    .unwrap();
    assert!(matches!(
        decoder.decode_fixture(&conflicting, Some(&config)),
        Err(DecodeError::ConfigurationMismatch)
    ));
    assert_eq!(session, decoder.session.as_ref().map(CFRetained::as_ptr));
    let frame = decoder
        .decode_fixture(HEVC_IDR, Some(&config))
        .unwrap()
        .into_fixture_frame()
        .unwrap();
    assert_native_red(&frame);
}

#[test]
fn hevc_output_retains_original_submission_token() {
    use std::sync::Arc;
    let token = Arc::new(crate::DecodeToken {
        timeline: crate::AccessUnitTimeline {
            connection_generation: 7,
            stream_generation: 9,
            frame_id: 42,
            source_pts_us: 123456,
            encoded_at_us: 123460,
            received_at_us: 123470,
            decode_submitted_at_us: 123480,
            kind: crate::FrameKind::Key,
        },
        decoder_generation: 11,
        config_revision: 13,
        stream_config: Some(Arc::new(hevc_config())),
    });
    let mut decoder = VideoToolboxDecoder::new();
    let outcome = decoder
        .submit(crate::DecodeSubmission {
            access_unit: HEVC_IDR,
            token: token.clone(),
        })
        .unwrap();
    assert!(outcome.refresh_accepted);
    assert_eq!(outcome.frames.len(), 1);
    assert!(Arc::ptr_eq(&token, &outcome.frames[0].token));
    assert_native_red(&outcome.frames[0].frame);
}

#[test]
fn unverified_hevc_leading_picture_sequences_are_rejected_before_session_creation() {
    for kind in [21u8, 8, 6] {
        let mut data = HEVC_IDR.to_vec();
        let mut offset = 0;
        while offset < data.len() {
            let count = u32::from_be_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
            offset += 4;
            if matches!((data[offset] >> 1) & 63, 19 | 20) {
                data[offset] = kind << 1;
                break;
            }
            offset += count;
        }
        let mut decoder = VideoToolboxDecoder::new();
        assert!(matches!(
            decoder.decode_fixture(&data, Some(&hevc_config())),
            Err(DecodeError::UnsupportedAccessUnit)
        ));
        assert!(decoder.session.is_none());
    }
}

#[test]
fn failed_native_session_preparation_preserves_working_decoder() {
    let mut decoder = VideoToolboxDecoder::new();
    let config = hevc_config();
    decoder.decode_fixture(HEVC_IDR, Some(&config)).unwrap();
    let session = decoder.session.as_ref().map(CFRetained::as_ptr);
    let record = decoder.configuration.as_ref().unwrap().record().to_vec();
    let (sps, _) = extract_sps_pps(H264_64X64_RED_IDR).unwrap();
    // The configuration container can frame this PPS, but native codec syntax
    // admission must reject the truncated body. Exercise the native preparation
    // boundary directly so shared source validation cannot hide a destructive reset.
    let invalid = CodecConfiguration::from_avc_parameter_sets(&sps, &[0x68, 0, 0]).unwrap();
    assert!(decoder.ensure_session(&invalid).is_err());
    assert_eq!(session, decoder.session.as_ref().map(CFRetained::as_ptr));
    assert_eq!(
        record.as_slice(),
        decoder.configuration.as_ref().unwrap().record().as_ref()
    );
    let frame = decoder
        .decode_fixture(HEVC_IDR, Some(&config))
        .unwrap()
        .into_fixture_frame()
        .unwrap();
    assert_native_red(&frame);
}
