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
    if config.sps.len().saturating_add(config.pps.len()) > 64 * 1024 {
        return Err(DecodeError::UnsupportedAccessUnit);
    }
    // StreamConfig carries raw parameter NAL payloads, never nested Annex B,
    // length prefixes or an entire access unit in the SPS field.
    if config.sps.len() < 4
        || config.sps[0] & 0x9f != 7
        || config.pps.len() < 2
        || config.pps[0] & 0x9f != 8
    {
        return Err(DecodeError::NotInitialized);
    }
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
            7 => Some(config.sps.as_slice()),
            8 => Some(config.pps.as_slice()),
            _ => None,
        };
        if expected.is_some_and(|expected| expected != nal) {
            return Err(DecodeError::ConfigurationMismatch);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use picoo_bitstream::avc::{annex_b_to_length_prefixed, extract_sps_pps};
    use picoo_testkit::{H264_1280X720_RED_IDR, H264_64X64_RED_IDR};

    #[test]
    fn declared_parameter_sets_cannot_be_replaced_by_an_in_band_update() {
        let (sps, pps) = extract_sps_pps(H264_64X64_RED_IDR).unwrap();
        let config = StreamConfig {
            codec: VideoCodec::Avc as i32,
            sps,
            pps,
            ..Default::default()
        };
        assert!(validate(H264_64X64_RED_IDR, Some(&config)).is_ok());
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
            au.extend_from_slice(H264_64X64_RED_IDR);
            assert!(matches!(
                validate(&au, Some(&config)),
                Err(DecodeError::ConfigurationMismatch)
            ));
        }
    }
}
