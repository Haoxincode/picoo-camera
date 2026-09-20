//! REQ-PICOO-BITSTREAM-005: native hardware fixtures with isolated parameter changes.
use picoo_bitstream::{AccessUnit, Codec, CodecConfiguration, NalFormat, NalLengthSize};

const WIRE: NalFormat = NalFormat::LengthPrefixed(NalLengthSize::Four);

fn fixtures() -> [(Codec, &'static [u8], &'static [u8]); 2] {
    [
        (
            Codec::Avc,
            include_bytes!("fixtures/avc-720p-config.bin"),
            include_bytes!("fixtures/avc-720p-idr.bin"),
        ),
        (
            Codec::Hevc,
            include_bytes!("fixtures/hevc-720p-config.bin"),
            include_bytes!("fixtures/hevc-720p-idr.bin"),
        ),
    ]
}

fn prepend(nal: &[u8], picture: &[u8]) -> Vec<u8> {
    let mut bytes = (nal.len() as u32).to_be_bytes().to_vec();
    bytes.extend_from_slice(nal);
    bytes.extend_from_slice(picture);
    bytes
}

#[test]
fn each_committed_parameter_is_accepted_but_an_isolated_change_is_rejected() {
    for (codec, record, bytes) in fixtures() {
        let config = CodecConfiguration::parse(codec, record.into()).unwrap();
        let picture = AccessUnit::parse(codec, WIRE, bytes).unwrap();
        config.validate_parameter_sets(&picture).unwrap();
        for parameter in config.vps().iter().chain(config.sps()).chain(config.pps()) {
            let matching = prepend(parameter, bytes);
            config
                .validate_parameter_sets(&AccessUnit::parse(codec, WIRE, &matching).unwrap())
                .unwrap();
            let mut altered = parameter.to_vec();
            // Keep NAL headers and framing intact; change the declared parameter payload.
            *altered.last_mut().unwrap() ^= 0x10;
            let mismatch = prepend(&altered, bytes);
            let parsed = AccessUnit::parse(codec, WIRE, &mismatch).unwrap();
            assert!(config.validate_parameter_sets(&parsed).is_err());
            // A rejection never mutates the committed record.
            assert_eq!(config.record().as_ref(), record);
            config.validate_parameter_sets(&picture).unwrap();
        }
    }
}

#[test]
fn valid_picture_of_another_codec_cannot_use_the_committed_record() {
    let cases = fixtures();
    for ((codec, record, _), (other, _, bytes)) in [(cases[0], cases[1]), (cases[1], cases[0])] {
        let config = CodecConfiguration::parse(codec, record.into()).unwrap();
        let picture = AccessUnit::parse(other, WIRE, bytes).unwrap();
        assert!(config.validate_parameter_sets(&picture).is_err());
    }
}

#[test]
fn source_geometry_is_checked_for_both_codecs_without_changing_the_record() {
    for (codec, record, _) in fixtures() {
        let config = CodecConfiguration::parse(codec, record.into()).unwrap();
        config.validate_visible_size(1280, 720).unwrap();
        for (width, height) in [(1920, 1080), (720, 1280), (0, 720)] {
            assert!(config.validate_visible_size(width, height).is_err());
            assert_eq!(config.record().as_ref(), record);
        }
    }
}
