use crate::control::*;
use crate::{MAX_DECODER_OFFERS, MAX_MEDIA_ACCESS_UNIT_BYTES};
use prost::Message;

fn offer(codec: VideoCodec, height: u32, fps: u32) -> DecoderOffer {
    DecoderOffer {
        format: Some(VideoFormat::sdr_709(
            codec,
            Resolution {
                width: if height == 720 { 1280 } else { 1920 },
                height,
            },
            FrameRate {
                numerator: fps,
                denominator: 1,
            },
            ColorRange::Limited,
        )),
        max_access_unit_bytes: MAX_MEDIA_ACCESS_UNIT_BYTES,
        max_level_idc: if codec == VideoCodec::Avc { 42 } else { 123 },
    }
}

#[test]
fn all_eight_product_combinations_roundtrip_without_cartesian_lists() {
    let offers = [VideoCodec::Avc, VideoCodec::Hevc]
        .into_iter()
        .flat_map(|codec| {
            [720, 1080].into_iter().flat_map(move |height| {
                [30, 60]
                    .into_iter()
                    .map(move |fps| offer(codec, height, fps))
            })
        })
        .collect();
    let caps = Capabilities { offers };
    assert_eq!(caps.validate(), Ok(()));
    assert_eq!(
        Capabilities::decode(caps.encode_to_vec().as_slice()).unwrap(),
        caps
    );
    for entry in &caps.offers {
        assert!(caps.supports(entry.format.as_ref().unwrap(), entry.max_level_idc, 4096));
    }
}

#[test]
fn capabilities_never_synthesize_unoffered_codec_size_rate_or_color() {
    let caps = Capabilities {
        offers: vec![
            offer(VideoCodec::Avc, 720, 30),
            offer(VideoCodec::Hevc, 1080, 60),
        ],
    };
    for unsupported in [
        offer(VideoCodec::Avc, 1080, 60),
        offer(VideoCodec::Hevc, 720, 30),
        offer(VideoCodec::Avc, 720, 60),
    ] {
        assert!(!caps.supports(
            unsupported.format.as_ref().unwrap(),
            unsupported.max_level_idc,
            4096
        ));
    }
    let mut different_color = caps.offers[0].format.unwrap();
    different_color.color.as_mut().unwrap().range = ColorRange::Full as i32;
    assert!(!caps.supports(&different_color, 42, 4096));
    let mut different_crop = caps.offers[0].format.unwrap();
    different_crop.visible_rect.as_mut().unwrap().width = 640;
    assert!(!caps.supports(&different_crop, 42, 4096));
}

#[test]
fn decoder_level_is_an_explicit_ceiling_for_the_same_combination() {
    let caps = Capabilities {
        offers: vec![offer(VideoCodec::Avc, 720, 30)],
    };
    let format = caps.offers[0].format.as_ref().unwrap();
    assert!(caps.supports(format, 31, 4096));
    assert!(caps.supports(format, 42, 4096));
    assert!(!caps.supports(format, 52, 4096));
    assert!(!caps.supports(format, 30, 4096));
    assert!(!caps.supports(format, 43, 4096));
    let mut insufficient = Capabilities {
        offers: vec![offer(VideoCodec::Hevc, 1080, 60)],
    };
    insufficient.offers[0].max_level_idc = 120;
    assert!(insufficient.validate().is_err());
}

#[test]
fn incomplete_unknown_or_out_of_bounds_formats_are_rejected() {
    let valid = offer(VideoCodec::Avc, 720, 30).format.unwrap();
    let mutations: &[fn(&mut VideoFormat)] = &[
        |f| f.codec = 0,
        |f| f.codec = 99,
        |f| f.profile = 0,
        |f| f.profile = VideoProfile::HevcMain as i32,
        |f| f.bit_depth = 10,
        |f| f.chroma = -1,
        |f| f.coded_size = None,
        |f| f.coded_size.as_mut().unwrap().height = 480,
        |f| f.visible_rect = None,
        |f| f.visible_rect.as_mut().unwrap().x = u32::MAX - 1,
        |f| f.visible_rect.as_mut().unwrap().width = 0,
        |f| f.visible_rect.as_mut().unwrap().width = 1279,
        |f| f.frame_rate = None,
        |f| f.frame_rate.as_mut().unwrap().denominator = 0,
        |f| f.frame_rate.as_mut().unwrap().numerator = 120,
        |f| f.color = None,
        |f| f.color.as_mut().unwrap().range = 0,
        |f| f.color.as_mut().unwrap().primaries = 99,
        |f| f.color.as_mut().unwrap().matrix = 0,
        |f| f.color.as_mut().unwrap().transfer = 99,
    ];
    for (index, mutate) in mutations.iter().enumerate() {
        let mut format = valid;
        mutate(&mut format);
        assert!(format.validate().is_err(), "mutation {index} accepted");
    }
}

#[test]
fn offer_count_duplicates_and_access_unit_budgets_are_bounded() {
    let entry = offer(VideoCodec::Avc, 720, 30);
    for offers in [vec![], vec![entry; 2], vec![entry; MAX_DECODER_OFFERS + 1]] {
        assert!(Capabilities { offers }.validate().is_err());
    }
    let mut caps = Capabilities {
        offers: vec![entry],
    };
    for budget in [0, MAX_MEDIA_ACCESS_UNIT_BYTES + 1, u32::MAX] {
        caps.offers[0].max_access_unit_bytes = budget;
        assert!(caps.validate().is_err());
    }
    caps.offers[0].max_access_unit_bytes = 4096;
    assert_eq!(caps.validate(), Ok(()));
    let format = caps.offers[0].format.as_ref().unwrap();
    assert!(caps.supports(format, 42, 4096));
    assert!(!caps.supports(format, 42, 4097));
    assert!(!caps.supports(format, 42, 0));
    caps.offers[0].format = None;
    assert!(caps.validate().is_err());
}

#[test]
fn padded_coded_geometry_is_distinct_from_the_formal_visible_image() {
    for codec in [VideoCodec::Avc, VideoCodec::Hevc] {
        let mut padded = offer(codec, 1080, 60);
        padded
            .format
            .as_mut()
            .unwrap()
            .coded_size
            .as_mut()
            .unwrap()
            .height = 1088;
        let caps = Capabilities {
            offers: vec![padded],
        };
        assert!(caps.validate().is_ok());
        let exact = caps.offers[0].format.unwrap();
        assert!(caps.supports(&exact, padded.max_level_idc, 1024));
        let unpadded = offer(codec, 1080, 60).format.unwrap();
        assert!(!caps.supports(&unpadded, padded.max_level_idc, 1024));
        let mut different_crop = exact;
        different_crop.visible_rect.as_mut().unwrap().y = 8;
        assert!(different_crop.validate().is_ok());
        assert!(!caps.supports(&different_crop, padded.max_level_idc, 1024));
        let mut padding_as_image = exact;
        padding_as_image.visible_rect.as_mut().unwrap().height = 1088;
        assert!(padding_as_image.validate().is_err());
    }
}

#[test]
fn cropped_visible_image_does_not_lower_the_coded_level_requirement() {
    let mut entry = offer(VideoCodec::Avc, 1080, 30);
    entry
        .format
        .as_mut()
        .unwrap()
        .coded_size
        .as_mut()
        .unwrap()
        .height = 1088;
    entry.format.as_mut().unwrap().visible_rect = Some(VisibleRect {
        x: 0,
        y: 0,
        width: 1280,
        height: 720,
    });
    entry.max_level_idc = 31;
    assert!(Capabilities {
        offers: vec![entry]
    }
    .validate()
    .is_err());
}
