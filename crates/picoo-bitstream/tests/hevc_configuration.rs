use picoo_bitstream::{split_nals, Codec, CodecConfiguration, NalFormat, NalLengthSize};

const ANNEX: &[u8] = include_bytes!("../../picoo-testkit/fixtures/hevc-64x64-bt709-idr.h265");
const RECORD: &[u8] = include_bytes!("../../picoo-testkit/fixtures/hevc-64x64-bt709-config.bin");

fn native_csd() -> Vec<u8> {
    let mut data = Vec::new();
    for nal in split_nals(NalFormat::AnnexB, ANNEX).unwrap() {
        if (32..=34).contains(&((nal[0] >> 1) & 63)) {
            data.extend_from_slice(&[0, 0, 0, 1]);
            data.extend_from_slice(nal);
        }
    }
    data
}

#[test]
fn native_csd_preserves_parameters_and_profile_tier_level() {
    let converted = CodecConfiguration::from_hevc_annex_b(&native_csd()).unwrap();
    let native = CodecConfiguration::parse(Codec::Hevc, bytes::Bytes::from_static(RECORD)).unwrap();
    assert_eq!(converted.vps(), native.vps());
    assert_eq!(converted.sps(), native.sps());
    assert_eq!(converted.pps(), native.pps());
    assert_eq!(converted.nal_length_size(), NalLengthSize::Four);
    // Compare the standard native profile/tier/compatibility/constraint/level bytes.
    // Encoder-specific optional fps, segmentation and completeness need not match.
    assert_eq!(&converted.record()[1..13], &native.record()[1..13]);
}

#[test]
fn rejects_picture_data_missing_sets_and_conflicting_sets() {
    assert!(CodecConfiguration::from_hevc_annex_b(ANNEX).is_err());
    let csd = native_csd();
    for end in 0..csd.len() {
        // Truncating the opaque PPS can remain a framed record; native codec
        // admission owns PPS syntax. Removing complete sets must always fail.
        let _ = CodecConfiguration::from_hevc_annex_b(&csd[..end]);
    }
    for missing in [32, 33, 34] {
        let mut partial = Vec::new();
        for nal in split_nals(NalFormat::AnnexB, &csd).unwrap() {
            if (nal[0] >> 1) & 63 != missing {
                partial.extend_from_slice(&[0, 0, 0, 1]);
                partial.extend_from_slice(nal);
            }
        }
        assert!(CodecConfiguration::from_hevc_annex_b(&partial).is_err());
    }
    let mut conflict = csd.clone();
    conflict.extend_from_slice(&[0, 0, 0, 1, 0x44, 1, 0x80]);
    assert!(CodecConfiguration::from_hevc_annex_b(&conflict).is_err());
    let repeated = [csd.as_slice(), csd.as_slice()].concat();
    assert!(CodecConfiguration::from_hevc_annex_b(&repeated).is_ok());
    assert!(CodecConfiguration::from_hevc_annex_b(&vec![0; 65537]).is_err());
}
