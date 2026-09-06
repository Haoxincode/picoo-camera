use crate::control::{ColorRange, StreamConfig, VideoCodec, VideoProfile};
use prost::Message;

// REQ-PICOO-PROTOCOL-016: source and offers use the same finite identity types.
#[test]
fn codec_profile_range_and_standard_level_roundtrip_without_string_labels() {
    for (codec, profile, level) in [
        (VideoCodec::Avc, VideoProfile::AvcHigh, 42),
        (VideoCodec::Hevc, VideoProfile::HevcMain, 123),
    ] {
        let config = StreamConfig {
            codec: codec as i32,
            profile: profile as i32,
            level_idc: level,
            color_range: ColorRange::Limited as i32,
            width: 1920,
            height: 1080,
            fps: 60,
            stream_epoch: 7,
            ..Default::default()
        };
        let decoded = StreamConfig::decode(config.encode_to_vec().as_slice()).unwrap();
        assert_eq!(decoded, config);
        assert_eq!(decoded.codec(), codec);
        assert_eq!(decoded.profile(), profile);
        assert_eq!(decoded.color_range(), ColorRange::Limited);
    }
}

#[test]
fn string_typed_source_identity_is_rejected_instead_of_converted() {
    // Length-delimited strings at the former codec/profile/level/range fields.
    for (field, value) in [
        (1u8, b"h264".as_slice()),
        (2, b"high"),
        (3, b"4.2"),
        (10, b"limited"),
    ] {
        let mut bytes = vec![(field << 3) | 2, value.len() as u8];
        bytes.extend_from_slice(value);
        assert!(StreamConfig::decode(bytes.as_slice()).is_err());
    }
}
