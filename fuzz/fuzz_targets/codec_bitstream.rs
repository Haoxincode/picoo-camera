#![no_main]
use libfuzzer_sys::fuzz_target;
use picoo_bitstream::{AccessUnit, Codec, CodecConfiguration, NalFormat, NalLengthSize};

fuzz_target!(|data: &[u8]| {
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
