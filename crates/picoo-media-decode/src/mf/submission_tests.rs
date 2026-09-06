//! The actual MF decoder must echo submission meaning across delayed output and reset.
use super::*;
use crate::{AccessUnitTimeline, DecodeSubmission, DecodeToken, FrameKind};
use windows::Win32::Media::MediaFoundation::{
    MFT_MESSAGE_COMMAND_DRAIN, MFT_MESSAGE_NOTIFY_END_OF_STREAM,
};

fn fixture() -> (Vec<u8>, StreamConfig) {
    let annex = picoo_testkit::AVC_64X64_BT709_IDR;
    let (sps, pps) = picoo_bitstream::avc::extract_sps_pps(annex).unwrap();
    let config = StreamConfig {
        codec: picoo_protocol::control::VideoCodec::Avc as i32,
        width: 64,
        height: 64,
        fps: 30,
        stream_epoch: 7,
        codec_configuration: picoo_bitstream::CodecConfiguration::from_avc_parameter_sets(
            &sps, &pps,
        )
        .unwrap()
        .record()
        .to_vec(),
        ..Default::default()
    };
    (
        picoo_bitstream::canonical_access_unit(
            picoo_bitstream::Codec::Avc,
            picoo_bitstream::NalFormat::AnnexB,
            annex,
        )
        .unwrap()
        .into_owned(),
        config,
    )
}

fn token(frame_id: u64, mut config: StreamConfig) -> Arc<DecodeToken> {
    config.rotation = if frame_id == 1 { 90 } else { 180 };
    Arc::new(DecodeToken {
        timeline: AccessUnitTimeline {
            connection_generation: 3,
            stream_generation: 7,
            frame_id,
            // Identical PTS must not collide: native submission time is an internal key.
            source_pts_us: 42,
            encoded_at_us: 100 + frame_id,
            received_at_us: 200 + frame_id,
            decode_submitted_at_us: 300 + frame_id,
            kind: FrameKind::Key,
        },
        decoder_generation: 5,
        config_revision: frame_id,
        stream_config: Some(Arc::new(config)),
    })
}

fn finish(decoder: &mut MfVideoDecoder) -> Vec<DecodedOutput> {
    unsafe {
        decoder
            .transform
            .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0)
            .unwrap();
        decoder
            .transform
            .ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0)
            .unwrap();
    }
    decoder.drain_frames().unwrap()
}

#[test]
fn native_sample_times_resolve_original_tokens_even_with_duplicate_source_pts() {
    let (wire, config) = fixture();
    let tokens = [token(1, config.clone()), token(2, config)];
    let mut decoder = MfVideoDecoder::software_diagnostic().unwrap();
    let mut frames = Vec::new();
    for token in &tokens {
        frames.extend(
            decoder
                .submit(DecodeSubmission {
                    access_unit: &wire,
                    token: token.clone(),
                })
                .unwrap()
                .frames,
        );
    }
    frames.extend(finish(&mut decoder));
    assert_eq!(frames.len(), 2);
    for (output, original) in frames.iter().zip(&tokens) {
        assert!(
            Arc::ptr_eq(&output.token, original),
            "output was relabeled with another input"
        );
        assert_eq!(
            (
                output.frame.description().width,
                output.frame.description().height
            ),
            (64, 64)
        );
    }
    assert!(decoder.pending.is_empty());
}

#[test]
fn reset_releases_pending_tokens_and_never_reuses_submission_times() {
    let (wire, config) = fixture();
    let first = token(1, config.clone());
    let retained = Arc::downgrade(&first);
    let mut decoder = MfVideoDecoder::software_diagnostic().unwrap();
    drop(
        decoder
            .submit(DecodeSubmission {
                access_unit: &wire,
                token: first,
            })
            .unwrap(),
    );
    let next_stamp = decoder.next_sample_time_100ns;
    decoder.reset().unwrap();
    assert!(retained.upgrade().is_none());
    assert!(decoder.pending.is_empty());
    assert_eq!(decoder.next_sample_time_100ns, next_stamp);
    let second = token(2, config);
    let mut frames = decoder
        .submit(DecodeSubmission {
            access_unit: &wire,
            token: second.clone(),
        })
        .unwrap()
        .frames;
    frames.extend(finish(&mut decoder));
    assert_eq!(frames.len(), 1);
    assert!(Arc::ptr_eq(&frames[0].token, &second));
    assert!(decoder.next_sample_time_100ns > next_stamp);
}
