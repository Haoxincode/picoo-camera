//! Canonical encoded-picture representation — REQ-PICOO-PROTOCOL-020.

use std::borrow::Cow;

use crate::{AccessUnit, BitstreamError, Codec, NalFormat, NalLengthSize};

/// Adapt one explicitly framed native picture to four-byte big-endian NAL lengths.
/// Already canonical input remains borrowed after bounded picture validation.
pub fn canonical_access_unit(
    codec: Codec,
    format: NalFormat,
    data: &[u8],
) -> Result<Cow<'_, [u8]>, BitstreamError> {
    let picture = AccessUnit::parse(codec, format, data)?;
    if format == NalFormat::LengthPrefixed(NalLengthSize::Four) {
        Ok(Cow::Borrowed(data))
    } else {
        picture.to_length_prefixed().map(Cow::Owned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_framing_normalizes_to_one_wire_form_without_copying_canonical_input() {
        for (codec, wire) in [
            (
                Codec::Avc,
                include_bytes!("../tests/fixtures/avc-720p-idr.bin").as_slice(),
            ),
            (
                Codec::Hevc,
                include_bytes!("../tests/fixtures/hevc-720p-idr.bin").as_slice(),
            ),
        ] {
            let canonical =
                canonical_access_unit(codec, NalFormat::LengthPrefixed(NalLengthSize::Four), wire)
                    .unwrap();
            assert!(matches!(canonical, Cow::Borrowed(_)));
            assert_eq!(canonical.as_ptr(), wire.as_ptr());
            let picture =
                AccessUnit::parse(codec, NalFormat::LengthPrefixed(NalLengthSize::Four), wire)
                    .unwrap();
            let annex = picture.to_annex_b().unwrap();
            assert_eq!(
                canonical_access_unit(codec, NalFormat::AnnexB, &annex)
                    .unwrap()
                    .as_ref(),
                wire
            );
            assert!(canonical_access_unit(
                codec,
                NalFormat::LengthPrefixed(NalLengthSize::Four),
                &annex
            )
            .is_err());
        }
    }

    #[test]
    fn missing_picture_and_incomplete_lengths_cannot_become_wire_access_units() {
        for bytes in [
            b"".as_slice(),
            &[0, 0, 0],
            &[0, 0, 0, 2, 0x65],
            &[0, 0, 0, 2, 0x67, 0x80],
        ] {
            assert!(canonical_access_unit(
                Codec::Avc,
                NalFormat::LengthPrefixed(NalLengthSize::Four),
                bytes
            )
            .is_err());
        }
    }
}
