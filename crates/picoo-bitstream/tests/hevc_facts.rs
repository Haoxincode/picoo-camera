//! REQ-PICOO-BITSTREAM-006: actual source facts and hostile SPS boundaries.
use picoo_bitstream::{Codec, CodecConfiguration, VideoSpsFacts};
use scuffle_bytes_util::BitWriter;
use scuffle_expgolomb::BitWriterExpGolombExt;

fn fixture() -> Vec<u8> {
    CodecConfiguration::parse(
        Codec::Hevc,
        include_bytes!("fixtures/hevc-720p-config.bin")
            .as_slice()
            .into(),
    )
    .unwrap()
    .sps()[0]
        .to_vec()
}

#[test]
fn hardware_hevc_source_preserves_geometry_without_inventing_aspect() {
    let facts = VideoSpsFacts::parse_hevc(&fixture()).unwrap();
    assert_eq!((facts.coded_width, facts.coded_height), (1280, 720));
    assert_eq!(
        (
            facts.visible_x,
            facts.visible_y,
            facts.visible_width,
            facts.visible_height
        ),
        (0, 0, 1280, 720)
    );
    assert_eq!(facts.pixel_aspect_ratio, None);
    assert_eq!(facts.chroma_location, 0);
}

#[test]
fn truncated_and_bit_mutated_hardware_sps_do_not_panic() {
    let original = fixture();
    for length in 0..original.len() {
        assert!(
            VideoSpsFacts::parse_hevc(&original[..length]).is_err(),
            "truncation {length}"
        );
    }
    for index in 0..original.len() {
        for bit in 0..8 {
            let mut data = original.clone();
            data[index] ^= 1 << bit;
            let _ = VideoSpsFacts::parse_hevc(&data);
        }
    }
    let mut trailing = original;
    trailing.push(0);
    assert!(VideoSpsFacts::parse_hevc(&trailing).is_err());
    assert!(VideoSpsFacts::parse_hevc(&vec![0; 65537]).is_err());
}

#[derive(Clone, Copy)]
enum Case {
    Valid,
    Cropped,
    ColorSquare,
    Crop,
    CodingBlock,
    TransformBlock,
    Scaling,
    Palette,
    Reorder,
}

fn synthetic(case: Case) -> Vec<u8> {
    let mut rbsp = Vec::new();
    let mut w = BitWriter::new(&mut rbsp);
    w.write_bits(0, 4).unwrap(); // VPS id
    w.write_bits(0, 3).unwrap(); // max sublayers minus one
    w.write_bit(true).unwrap();
    w.write_bits(1, 8).unwrap(); // Main profile, main tier
    w.write_bits(0x60000000, 32).unwrap();
    w.write_bits(0xb00000000000, 48).unwrap(); // progressive, frame-only
    w.write_bits(93, 8).unwrap();
    let (width, height) = if matches!(case, Case::Cropped) {
        (1920, 1088)
    } else {
        (1280, 720)
    };
    for value in [0, 1, width, height] {
        w.write_exp_golomb(value).unwrap();
    }
    w.write_bit(matches!(case, Case::Crop | Case::Cropped))
        .unwrap();
    if matches!(case, Case::Crop) {
        for value in [u64::MAX - 1, 0, 0, 0] {
            w.write_exp_golomb(value).unwrap();
        }
    }
    if matches!(case, Case::Cropped) {
        for value in [0, 0, 0, 4] {
            w.write_exp_golomb(value).unwrap();
        }
    }
    for value in [0, 0, 4] {
        w.write_exp_golomb(value).unwrap();
    }
    w.write_bit(false).unwrap(); // sublayer ordering
    for value in [1, u64::from(matches!(case, Case::Reorder)), 0] {
        w.write_exp_golomb(value).unwrap();
    }
    let cb = if matches!(case, Case::CodingBlock) {
        u64::MAX - 1
    } else {
        0
    };
    let tb = if matches!(case, Case::TransformBlock) {
        u64::MAX - 1
    } else {
        0
    };
    for value in [cb, 3, tb, 3, 0, 0] {
        w.write_exp_golomb(value).unwrap();
    }
    w.write_bit(matches!(case, Case::Scaling)).unwrap();
    if matches!(case, Case::Scaling) {
        w.write_bit(true).unwrap(); // scaling list present
        w.write_bit(false).unwrap(); // predicted matrix
        w.write_exp_golomb(1).unwrap(); // impossible reference before matrix zero
                                        // The remaining matrices use defaults; reject because the reference is invalid.
        for _ in 1..20 {
            w.write_bit(false).unwrap();
            w.write_exp_golomb(0).unwrap();
        }
    }
    for _ in 0..3 {
        w.write_bit(false).unwrap();
    } // AMP, SAO, PCM
    w.write_exp_golomb(0).unwrap(); // short-term reference sets
    for _ in 0..3 {
        w.write_bit(false).unwrap();
    } // long-term, temporal MVP, smoothing
    w.write_bit(matches!(case, Case::ColorSquare)).unwrap();
    if matches!(case, Case::ColorSquare) {
        w.write_bit(true).unwrap(); // aspect present
        w.write_bits(1, 8).unwrap(); // square
        w.write_bit(false).unwrap(); // overscan absent
        w.write_bit(true).unwrap(); // video signal type present
        w.write_bits(5, 3).unwrap(); // unspecified video format
        w.write_bit(false).unwrap(); // limited range
        w.write_bit(true).unwrap(); // colour description present
        for _ in 0..3 {
            w.write_bits(1, 8).unwrap();
        } // BT.709
        for _ in 0..7 {
            w.write_bit(false).unwrap();
        } // chroma, neutral, field, frame info, window, timing, restriction
    }
    w.write_bit(matches!(case, Case::Palette)).unwrap();
    if matches!(case, Case::Palette) {
        w.write_bits(0x10, 8).unwrap(); // SCC extension only
        w.write_bit(false).unwrap(); // current picture ref
        w.write_bit(true).unwrap(); // palette enabled
        w.write_exp_golomb(65).unwrap(); // larger than standard palette capacity
        w.write_exp_golomb(0).unwrap();
        w.write_bit(false).unwrap();
        w.write_bits(0, 2).unwrap();
        w.write_bit(false).unwrap();
    }
    w.write_bit(true).unwrap(); // rbsp stop bit
    w.finish().unwrap();
    let mut nal = vec![0x42, 1];
    let mut zeros = 0;
    for byte in rbsp {
        if zeros == 2 && byte <= 3 {
            nal.push(3);
            zeros = 0;
        }
        nal.push(byte);
        zeros = if byte == 0 { zeros + 1 } else { 0 };
    }
    nal
}

#[test]
fn malicious_fields_are_rejected_before_arithmetic_or_allocation() {
    VideoSpsFacts::parse_hevc(&synthetic(Case::Valid)).unwrap();
    for case in [
        Case::Crop,
        Case::CodingBlock,
        Case::TransformBlock,
        Case::Scaling,
        Case::Palette,
        Case::Reorder,
    ] {
        let nal = synthetic(case);
        if !matches!(case, Case::Reorder) {
            assert!(scuffle_h265::SpsNALUnit::parse(nal.as_slice()).is_err());
        }
        assert!(VideoSpsFacts::parse_hevc(&nal).is_err());
    }
}

#[test]
fn conformance_crop_and_explicit_vui_remain_distinct_source_facts() {
    let cropped = VideoSpsFacts::parse_hevc(&synthetic(Case::Cropped)).unwrap();
    assert_eq!((cropped.coded_width, cropped.coded_height), (1920, 1088));
    assert_eq!(
        (cropped.visible_width, cropped.visible_height),
        (1920, 1080)
    );
    let color = VideoSpsFacts::parse_hevc(&synthetic(Case::ColorSquare)).unwrap();
    assert_eq!(color.pixel_aspect_ratio, Some((1, 1)));
    assert_eq!(
        color.color,
        Some(picoo_bitstream::VideoColorFacts {
            full_range: false,
            primaries: 1,
            transfer: 1,
            matrix: 1,
        })
    );
}
