//! Committed AVC configuration is authoritative — REQ-PICOO-MEDIA-029.
use crate::DecodeError;
use picoo_bitstream::{split_nals, NalFormat, NalLengthSize};
use picoo_protocol::control::{StreamConfig, VideoCodec};

pub(crate) fn validate(
    access_unit: &[u8],
    config: Option<&StreamConfig>,
) -> Result<(), DecodeError> {
    let Some(config) = config else {
        return Ok(());
    };
    if config.codec != VideoCodec::Avc as i32 {
        return Err(DecodeError::UnsupportedAccessUnit);
    }
    let configuration = configuration(config)?;
    // The existing native adapter accepts Annex B or four-byte lengths. Typed
    // framing will replace this adapter detection with the source format contract.
    let format = if access_unit.starts_with(&[0, 0, 1]) || access_unit.starts_with(&[0, 0, 0, 1]) {
        NalFormat::AnnexB
    } else {
        NalFormat::LengthPrefixed(NalLengthSize::Four)
    };
    let nals = split_nals(format, access_unit).map_err(|_| DecodeError::UnsupportedAccessUnit)?;
    for nal in nals {
        let expected = match nal[0] & 0x1f {
            7 => Some(configuration.sps()),
            8 => Some(configuration.pps()),
            _ => None,
        };
        if expected.is_some_and(|expected| !expected.iter().any(|set| set.as_ref() == nal)) {
            return Err(DecodeError::ConfigurationMismatch);
        }
    }
    Ok(())
}

/// Standard record interpretation stays in picoo-bitstream, shared by native adapters.
pub(crate) fn configuration(
    config: &StreamConfig,
) -> Result<picoo_bitstream::CodecConfiguration, DecodeError> {
    if config.codec != VideoCodec::Avc as i32 {
        return Err(DecodeError::UnsupportedAccessUnit);
    }
    picoo_bitstream::CodecConfiguration::parse(
        picoo_bitstream::Codec::Avc,
        config.codec_configuration.clone().into(),
    )
    .map_err(|_| DecodeError::NotInitialized)
}

/// Native decoders that accept an Annex B sequence header receive every declared set.
#[cfg(any(
    windows,
    all(not(target_vendor = "apple"), any(test, feature = "test-codecs"))
))]
pub(crate) fn sequence_header(config: &StreamConfig) -> Result<Vec<u8>, DecodeError> {
    let config = configuration(config)?;
    let mut header = Vec::new();
    for set in config.sps().iter().chain(config.pps()) {
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
        assert!(matches!(
            configuration(&config),
            Err(DecodeError::UnsupportedAccessUnit)
        ));
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
        assert!(validate(AVC_64X64_BT709_IDR, Some(&config)).is_ok());
        for au in [
            H264_1280X720_RED_IDR.to_vec(),
            annex_b_to_length_prefixed(H264_1280X720_RED_IDR).unwrap(),
        ] {
            assert!(matches!(
                validate(&au, Some(&config)),
                Err(DecodeError::ConfigurationMismatch)
            ));
        }
        for kind in [7, 8] {
            // Even an isolated parameter update must match; requiring a pair
            // would miss one changed parameter set in an otherwise valid AU.
            let mut au = vec![0, 0, 0, 1, 0x60 | kind, 0xaa, 0x80];
            au.extend_from_slice(AVC_64X64_BT709_IDR);
            assert!(matches!(
                validate(&au, Some(&config)),
                Err(DecodeError::ConfigurationMismatch)
            ));
        }
    }
}
