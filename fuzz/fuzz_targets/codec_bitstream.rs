#![no_main]
use libfuzzer_sys::fuzz_target;
use picoo_bitstream::{AccessUnit, Codec, CodecConfiguration, NalFormat, NalLengthSize};

fuzz_target!(|data: &[u8]| {
    if data.len() >= 2 {
        let split = usize::from(u16::from_be_bytes([data[0], data[1]])).min(data.len() - 2);
        let (sps, pps) = data[2..].split_at(split);
        if let Ok(config) = CodecConfiguration::from_avc_parameter_sets(sps, pps) {
            let roundtrip = CodecConfiguration::parse(Codec::Avc, config.record().clone()).unwrap();
            assert_eq!(roundtrip.sps(), config.sps());
            assert_eq!(roundtrip.pps(), config.pps());
        }
    }
    for codec in [Codec::Avc, Codec::Hevc] {
        let _ = CodecConfiguration::parse(codec, bytes::Bytes::copy_from_slice(data));
        for format in [NalFormat::AnnexB, NalFormat::LengthPrefixed(NalLengthSize::One),
                       NalFormat::LengthPrefixed(NalLengthSize::Two), NalFormat::LengthPrefixed(NalLengthSize::Four)] {
            if let Ok(au) = AccessUnit::parse(codec, format, data) {
                if let Ok(encoded) = au.to_length_prefixed() {
                    let parsed = AccessUnit::parse(codec, NalFormat::LengthPrefixed(NalLengthSize::Four), &encoded).unwrap();
                    assert_eq!(parsed.picture(), au.picture());
                    assert_eq!(parsed.nals(), au.nals());
                }
            }
        }
    }
});
