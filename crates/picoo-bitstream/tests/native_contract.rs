use bytes::Bytes;
use picoo_bitstream::{
    AccessUnit, Codec, CodecConfiguration, NalFormat, NalLengthSize, PictureKind, RandomAccessPoint,
};

#[test]
fn hardware_avc_hevc_configuration_and_au_remain_codec_specific() {
    // REQ-PICOO-BITSTREAM-002: produced by real VideoToolbox hardware on M4.
    for (codec, config, au, rap) in [
        (
            Codec::Avc,
            &include_bytes!("fixtures/avc-720p-config.bin")[..],
            &include_bytes!("fixtures/avc-720p-idr.bin")[..],
            RandomAccessPoint::AvcIdr,
        ),
        (
            Codec::Hevc,
            &include_bytes!("fixtures/hevc-720p-config.bin")[..],
            &include_bytes!("fixtures/hevc-720p-idr.bin")[..],
            RandomAccessPoint::HevcIdr,
        ),
    ] {
        let config = CodecConfiguration::parse(codec, Bytes::copy_from_slice(config)).unwrap();
        assert_eq!(config.codec(), codec);
        assert_eq!(config.nal_length_size(), NalLengthSize::Four);
        assert!(!config.sps().is_empty());
        assert!(!config.pps().is_empty());
        assert_eq!(config.vps().is_empty(), codec == Codec::Avc);
        let parsed = AccessUnit::parse(
            codec,
            NalFormat::LengthPrefixed(config.nal_length_size()),
            au,
        )
        .unwrap();
        assert_eq!(parsed.picture().kind, PictureKind::RandomAccess(rap));
        assert!(!parsed.picture().discardable);
        assert_eq!(parsed.to_length_prefixed().unwrap(), au);
        let annex = parsed.to_annex_b().unwrap();
        let roundtrip = AccessUnit::parse(codec, NalFormat::AnnexB, &annex).unwrap();
        assert_eq!(roundtrip.picture(), parsed.picture());
        assert_eq!(roundtrip.to_length_prefixed().unwrap(), au);
    }
}
#[test]
fn cra_rasl_and_radl_have_distinct_recovery_meaning() {
    for (kind, picture, discardable) in [
        (
            21,
            PictureKind::RandomAccess(RandomAccessPoint::HevcCra),
            false,
        ),
        (8, PictureKind::HevcRasl, true),
        (9, PictureKind::HevcRasl, false),
        (6, PictureKind::HevcRadl, true),
        (7, PictureKind::HevcRadl, false),
    ] {
        let au = [0, 0, 0, 3, kind << 1, 1, 0x80];
        let parsed = AccessUnit::parse(
            Codec::Hevc,
            NalFormat::LengthPrefixed(NalLengthSize::Four),
            &au,
        )
        .unwrap();
        assert_eq!(parsed.picture().kind, picture);
        assert_eq!(parsed.picture().discardable, discardable);
    }
}
#[test]
fn reject_unsupported_irap_multilayer_and_multiple_pictures() {
    for nal in [
        [16 << 1, 1, 0x80],
        [22 << 1, 1, 0x80],
        [19 << 1, 9, 0x80],
        [19 << 1, 0, 0x80],
        [19 << 1, 2, 0x80],
        [19 << 1, 1, 0x40],
    ] {
        let mut au = vec![0, 0, 0, 3];
        au.extend(nal);
        assert!(AccessUnit::parse(
            Codec::Hevc,
            NalFormat::LengthPrefixed(NalLengthSize::Four),
            &au
        )
        .is_err());
    }
    for (codec, nal) in [
        (Codec::Avc, vec![0x65, 0x80]),
        (Codec::Hevc, vec![38, 1, 0x80]),
    ] {
        let mut au = Vec::new();
        for _ in 0..2 {
            au.extend((nal.len() as u32).to_be_bytes());
            au.extend(&nal);
        }
        assert!(
            AccessUnit::parse(codec, NalFormat::LengthPrefixed(NalLengthSize::Four), &au).is_err()
        );
    }
}
#[test]
fn length_is_never_guessed_and_malformed_boundaries_are_rejected() {
    let ambiguous = [0, 0, 0, 1, 0x06]; // Valid one-byte NAL in explicit framing, not a picture.
    assert_eq!(
        picoo_bitstream::split_nals(NalFormat::LengthPrefixed(NalLengthSize::Four), &ambiguous)
            .unwrap(),
        vec![&[0x06][..]]
    );
    for input in [
        vec![],
        vec![0, 0, 0],
        vec![0, 0, 0, 0],
        vec![255, 255, 255, 255],
        vec![0, 0, 0, 4, 0x65, 0x80],
    ] {
        assert!(AccessUnit::parse(
            Codec::Avc,
            NalFormat::LengthPrefixed(NalLengthSize::Four),
            &input
        )
        .is_err());
    }
    assert!(picoo_bitstream::split_nals(NalFormat::AnnexB, &[9, 0, 0, 1, 0x65, 0x80]).is_err());
    assert!(
        picoo_bitstream::split_nals(NalFormat::AnnexB, &[0, 0, 1, 0, 0, 1, 0x65, 0x80]).is_err()
    );
    let many = [0, 0, 0, 1, 0x06].repeat(257);
    assert!(
        picoo_bitstream::split_nals(NalFormat::LengthPrefixed(NalLengthSize::Four), &many).is_err()
    );
}
#[test]
fn configuration_truncations_and_hostile_array_sizes_are_rejected() {
    for (codec, record) in [
        (
            Codec::Avc,
            &include_bytes!("fixtures/avc-720p-config.bin")[..],
        ),
        (
            Codec::Hevc,
            &include_bytes!("fixtures/hevc-720p-config.bin")[..],
        ),
    ] {
        for end in 0..record.len() {
            // AVC High records may legally omit the four-byte optional extension.
            if codec == Codec::Avc && end == record.len() - 4 {
                let short =
                    CodecConfiguration::parse(codec, Bytes::copy_from_slice(&record[..end]))
                        .unwrap();
                let full =
                    CodecConfiguration::parse(codec, Bytes::copy_from_slice(record)).unwrap();
                assert_eq!(short.sps(), full.sps());
                assert_eq!(short.pps(), full.pps());
                continue;
            }
            assert!(
                CodecConfiguration::parse(codec, Bytes::copy_from_slice(&record[..end])).is_err(),
                "{codec:?} at {end}"
            );
        }
        let mut garbage = record.to_vec();
        garbage.push(0);
        assert!(CodecConfiguration::parse(codec, garbage.into()).is_err());
    }
    let mut hevc = include_bytes!("fixtures/hevc-720p-config.bin").to_vec();
    hevc[24] = 255;
    hevc[25] = 255;
    assert!(CodecConfiguration::parse(Codec::Hevc, hevc.into()).is_err());
}
