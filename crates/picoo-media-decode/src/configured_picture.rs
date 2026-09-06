//! Committed codec configuration is authoritative — REQ-PICOO-MEDIA-029.
use crate::DecodeError;
use picoo_bitstream::{AccessUnit, Codec, NalFormat, NalLengthSize};
use picoo_protocol::control::{StreamConfig, VideoCodec};

pub(crate) fn validate<'a>(
    codec: Codec,
    access_unit: &'a [u8],
    config: Option<&StreamConfig>,
) -> Result<AccessUnit<'a>, DecodeError> {
    if config.is_some_and(|config| configured_codec(config).ok() != Some(codec)) {
        return Err(DecodeError::UnsupportedAccessUnit);
    }
    let picture = AccessUnit::parse(
        codec,
        NalFormat::LengthPrefixed(NalLengthSize::Four),
        access_unit,
    )
    .map_err(|_| DecodeError::UnsupportedAccessUnit)?;
    let Some(config) = config else {
        return Ok(picture);
    };
    let configuration = configuration(config)?;
    configuration
        .validate_parameter_sets(&picture)
        .map_err(|_| DecodeError::ConfigurationMismatch)?;
    Ok(picture)
}

pub(crate) fn configured_codec(config: &StreamConfig) -> Result<Codec, DecodeError> {
    match VideoCodec::try_from(config.codec) {
        Ok(VideoCodec::Avc) => Ok(Codec::Avc),
        Ok(VideoCodec::Hevc) => Ok(Codec::Hevc),
        _ => Err(DecodeError::UnsupportedAccessUnit),
    }
}

/// Standard record interpretation stays in picoo-bitstream, shared by native adapters.
pub(crate) fn configuration(
    config: &StreamConfig,
) -> Result<picoo_bitstream::CodecConfiguration, DecodeError> {
    let configuration = picoo_bitstream::CodecConfiguration::parse(
        configured_codec(config)?,
        config.codec_configuration.clone().into(),
    )
    .map_err(|_| DecodeError::NotInitialized)?;
    if configuration.nal_length_size() != NalLengthSize::Four {
        return Err(DecodeError::UnsupportedAccessUnit);
    }
    Ok(configuration)
}

#[cfg(any(target_os = "macos", all(windows, feature = "windows-mf")))]
pub(crate) fn source_facts(
    configuration: &picoo_bitstream::CodecConfiguration,
) -> Result<picoo_bitstream::VideoSpsFacts, DecodeError> {
    let parse = match configuration.codec() {
        Codec::Avc => picoo_bitstream::VideoSpsFacts::parse_avc,
        Codec::Hevc => picoo_bitstream::VideoSpsFacts::parse_hevc,
    };
    let mut facts = None;
    for sps in configuration.sps() {
        let current = parse(sps).map_err(|error| DecodeError::Platform(error.to_string()))?;
        if facts.is_some_and(|previous| previous != current) {
            return Err(DecodeError::ConfigurationMismatch);
        }
        facts = Some(current);
    }
    facts.ok_or(DecodeError::NotInitialized)
}

/// Native decoders that accept an Annex B sequence header receive every declared set.
#[cfg(any(
    windows,
    all(not(target_vendor = "apple"), any(test, feature = "test-codecs"))
))]
pub(crate) fn sequence_header(config: &StreamConfig) -> Result<Vec<u8>, DecodeError> {
    let config = configuration(config)?;
    let mut header = Vec::new();
    for set in config.vps().iter().chain(config.sps()).chain(config.pps()) {
        header.extend_from_slice(&[0, 0, 0, 1]);
        header.extend_from_slice(set);
    }
    Ok(header)
}

#[cfg(test)]
mod tests {
    use super::*;
    use picoo_bitstream::avc::{annex_b_to_length_prefixed, extract_sps_pps};
    use picoo_testkit::{AVC_64X64_BT709_IDR, H264_1280X720_RED_IDR};

    #[test]
    fn missing_truncated_and_wrong_codec_records_are_rejected() {
        let (sps, pps) = extract_sps_pps(AVC_64X64_BT709_IDR).unwrap();
        let record = picoo_bitstream::CodecConfiguration::from_avc_parameter_sets(&sps, &pps)
            .unwrap()
            .record()
            .to_vec();
        for length in 0..record.len() {
            let config = StreamConfig {
                codec: VideoCodec::Avc as i32,
                codec_configuration: record[..length].to_vec(),
                ..Default::default()
            };
            assert!(configuration(&config).is_err(), "truncation at {length}");
        }
        let config = StreamConfig {
            codec: VideoCodec::Hevc as i32,
            codec_configuration: record,
            ..Default::default()
        };
        assert!(configuration(&config).is_err());
    }

    #[test]
    fn declared_parameter_sets_cannot_be_replaced_by_an_in_band_update() {
        let (sps, pps) = extract_sps_pps(AVC_64X64_BT709_IDR).unwrap();
        let config = StreamConfig {
            codec: VideoCodec::Avc as i32,
            codec_configuration: picoo_bitstream::CodecConfiguration::from_avc_parameter_sets(
                &sps, &pps,
            )
            .unwrap()
            .record()
            .to_vec(),
            ..Default::default()
        };
        assert!(validate(
            Codec::Avc,
            &annex_b_to_length_prefixed(AVC_64X64_BT709_IDR).unwrap(),
            Some(&config)
        )
        .is_ok());
        assert!(matches!(
            validate(Codec::Avc, AVC_64X64_BT709_IDR, Some(&config)),
            Err(DecodeError::UnsupportedAccessUnit)
        ));
        assert!(matches!(
            validate(
                Codec::Avc,
                &annex_b_to_length_prefixed(H264_1280X720_RED_IDR).unwrap(),
                Some(&config)
            ),
            Err(DecodeError::ConfigurationMismatch)
        ));
        for kind in [7, 8] {
            // Even an isolated parameter update must match; requiring a pair
            // would miss one changed parameter set in an otherwise valid AU.
            let mut au = vec![0, 0, 0, 1, 0x60 | kind, 0xaa, 0x80];
            au.extend_from_slice(AVC_64X64_BT709_IDR);
            assert!(matches!(
                validate(
                    Codec::Avc,
                    &annex_b_to_length_prefixed(&au).unwrap(),
                    Some(&config)
                ),
                Err(DecodeError::ConfigurationMismatch)
            ));
        }
    }
}
