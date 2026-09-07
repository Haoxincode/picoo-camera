use super::*;
use crate::control::{Capabilities, DecoderOffer, VideoProfile};

fn source(codec: Codec, bytes: &[u8], height: u32, fps: u32) -> StreamConfig {
    let record = CodecConfiguration::parse(codec, bytes.to_vec().into()).unwrap();
    StreamConfig {
        codec: match codec {
            Codec::Avc => VideoCodec::Avc,
            Codec::Hevc => VideoCodec::Hevc,
        } as i32,
        profile: match codec {
            Codec::Avc => VideoProfile::AvcHigh,
            Codec::Hevc => VideoProfile::HevcMain,
        } as i32,
        level_idc: u32::from(record.level_idc()),
        width: if height == 720 { 1280 } else { 1920 },
        height,
        fps,
        color_range: ColorRange::Limited as i32,
        codec_configuration: bytes.to_vec(),
        ..Default::default()
    }
}

#[test]
fn actual_hardware_records_preserve_padding_and_exact_offer_membership() {
    macro_rules! sample {
        ($codec:ident, $file:literal, $height:expr, $fps:expr) => {
            source(
                Codec::$codec,
                include_bytes!(concat!(
                    "../../picoo-media-decode/probes/apple-native-formats/",
                    $file,
                    ".config"
                )),
                $height,
                $fps,
            )
        };
    }
    for config in [
        sample!(Avc, "1-720-30", 720, 30),
        sample!(Avc, "1-720-60", 720, 60),
        sample!(Avc, "1-1080-30", 1080, 30),
        sample!(Avc, "1-1080-60", 1080, 60),
        sample!(Hevc, "2-720-30", 720, 30),
        sample!(Hevc, "2-720-60", 720, 60),
        sample!(Hevc, "2-1080-30", 1080, 30),
        sample!(Hevc, "2-1080-60", 1080, 60),
    ] {
        let format = config.validated_video_format().unwrap();
        assert_eq!(
            format.coded_size.as_ref().unwrap().height,
            if config.height == 1080 { 1088 } else { 720 }
        );
        assert_eq!(format.visible_rect.as_ref().unwrap().height, config.height);
        let caps = Capabilities {
            offers: vec![DecoderOffer {
                format: Some(format),
                max_level_idc: config.level_idc,
                max_access_unit_bytes: 1024,
            }],
        };
        assert!(caps.supports(&format, config.level_idc, 1024));
        assert!(!caps.supports(&format, config.level_idc, 1025));
        let mut other = format;
        other.frame_rate.as_mut().unwrap().numerator = if config.fps == 30 { 60 } else { 30 };
        assert!(!caps.supports(&other, config.level_idc, 1024));
        for mutate in [
            |c: &mut StreamConfig| c.profile = VideoProfile::Unspecified as i32,
            |c: &mut StreamConfig| c.level_idc += 1,
            |c: &mut StreamConfig| c.width += 2,
            |c: &mut StreamConfig| c.height += 2,
            |c: &mut StreamConfig| c.color_range = ColorRange::Full as i32,
            |c: &mut StreamConfig| c.rotation = 91,
            |c: &mut StreamConfig| c.fps = 24,
            |c: &mut StreamConfig| c.codec = VideoCodec::Unspecified as i32,
        ] {
            let mut invalid = config.clone();
            mutate(&mut invalid);
            assert!(invalid.validated_video_format().is_err());
        }
    }
}

#[test]
fn missing_color_cannot_be_filled_from_wire_labels() {
    let (sps, pps) = picoo_bitstream::avc::extract_sps_pps(include_bytes!(
        "../../picoo-testkit/fixtures/avc-1080p-red-idr.h264"
    ))
    .unwrap();
    let record = CodecConfiguration::from_avc_parameter_sets(&sps, &pps).unwrap();
    assert!(record.source_facts().unwrap().color.is_none());
    let config = source(Codec::Avc, record.record(), 1080, 30);
    assert_eq!(
        config.validated_video_format(),
        Err(MediaFormatError("missing source color"))
    );
}

#[test]
fn xiaomi_hevc_720_preserves_736_storage_and_720_presentation() {
    for (fps, bytes) in [
        (
            30,
            include_bytes!("../../picoo-media-decode/probes/xiaomi-native-formats/2-720-30.config")
                .as_slice(),
        ),
        (
            60,
            include_bytes!("../../picoo-media-decode/probes/xiaomi-native-formats/2-720-60.config")
                .as_slice(),
        ),
    ] {
        let config = source(Codec::Hevc, bytes, 720, fps);
        let format = config.validated_video_format().unwrap();
        assert_eq!(format.tier, crate::control::VideoTier::HevcHigh as i32);
        assert_eq!(
            format.coded_size.unwrap(),
            Resolution {
                width: 1280,
                height: 736
            }
        );
        assert_eq!(
            format.visible_rect.unwrap(),
            VisibleRect {
                x: 0,
                y: 0,
                width: 1280,
                height: 720
            }
        );
        let caps = Capabilities {
            offers: vec![DecoderOffer {
                format: Some(format),
                max_level_idc: config.level_idc,
                max_access_unit_bytes: 2048,
            }],
        };
        assert!(caps.supports(&format, config.level_idc, 1024));
        let mut unpadded = format;
        unpadded.coded_size.as_mut().unwrap().height = 720;
        assert!(!caps.supports(&unpadded, config.level_idc, 1024));
    }
}

#[test]
fn hevc_record_cannot_lie_about_sps_tier_or_level() {
    let bytes =
        include_bytes!("../../picoo-media-decode/probes/xiaomi-native-formats/2-720-30.config");
    for (index, mask) in [(1, 0x20), (12, 1)] {
        let mut changed = bytes.to_vec();
        changed[index] ^= mask;
        let record = CodecConfiguration::parse(Codec::Hevc, changed.into()).unwrap();
        assert!(record.source_facts().is_err());
        assert!(VideoFormat::from_codec_configuration(&record, 30).is_err());
    }
}
