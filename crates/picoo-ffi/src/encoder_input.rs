//! REQ-PICOO-NEXT-023/025: native sync hints cannot commit an unverified RAP.
use picoo_bitstream::{
    AccessUnit, BitstreamError, Codec, NalFormat, NalLengthSize, PictureKind, RandomAccessPoint,
};
use std::borrow::Cow;

pub(crate) fn canonical(
    codec: Codec,
    format: NalFormat,
    data: &[u8],
    keyframe_hint: bool,
) -> Result<Cow<'_, [u8]>, BitstreamError> {
    let picture = AccessUnit::parse(codec, format, data)?;
    let keyframe = match picture.picture().kind {
        PictureKind::RandomAccess(RandomAccessPoint::AvcIdr | RandomAccessPoint::HevcIdr) => true,
        PictureKind::Trailing => false,
        // No platform may equate CRA and IDR without a verified leading-picture policy.
        PictureKind::RandomAccess(RandomAccessPoint::HevcCra)
        | PictureKind::HevcRasl
        | PictureKind::HevcRadl => {
            return Err(BitstreamError::Unsupported(
                "native source requires closed IDR sequence",
            ));
        }
    };
    if keyframe_hint != keyframe {
        return Err(BitstreamError::Malformed(
            "native keyframe hint differs from picture",
        ));
    }
    if format == NalFormat::LengthPrefixed(NalLengthSize::Four) {
        Ok(Cow::Borrowed(data))
    } else {
        picture.to_length_prefixed().map(Cow::Owned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const HEVC: &[u8] = include_bytes!("../../picoo-testkit/fixtures/hevc-64x64-bt709-idr.bin");
    const AVC: &[u8] = include_bytes!("../../picoo-testkit/fixtures/avc-64x64-bt709-idr.h264");

    #[test]
    fn platform_idr_hints_match_actual_codec_and_canonical_framing() {
        for (codec, format, bytes) in [
            (
                Codec::Hevc,
                NalFormat::LengthPrefixed(NalLengthSize::Four),
                HEVC,
            ),
            (Codec::Avc, NalFormat::AnnexB, AVC),
        ] {
            let accepted = canonical(codec, format, bytes, true).unwrap();
            assert!(canonical(codec, format, bytes, false).is_err());
            let borrowed = canonical(
                codec,
                NalFormat::LengthPrefixed(NalLengthSize::Four),
                &accepted,
                true,
            )
            .unwrap();
            assert_eq!(borrowed.as_ptr(), accepted.as_ptr());
        }
    }

    #[test]
    fn delta_header_cannot_be_promoted_by_a_native_sync_flag() {
        // Header-level evidence; native Decoder remains responsible for full slice syntax.
        let delta = [0, 0, 0, 2, 0x41, 0x80];
        let format = NalFormat::LengthPrefixed(NalLengthSize::Four);
        assert!(canonical(Codec::Avc, format, &delta, false).is_ok());
        assert!(canonical(Codec::Avc, format, &delta, true).is_err());
    }

    #[test]
    fn hevc_native_sync_flag_cannot_promote_cra_or_leading_pictures() {
        for nal_type in [21, 8, 9, 6, 7] {
            let mut data = HEVC.to_vec();
            let mut offset = 0;
            let mut changed = false;
            while offset < data.len() {
                let length =
                    u32::from_be_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
                offset += 4;
                if matches!((data[offset] >> 1) & 63, 19 | 20) {
                    data[offset] = nal_type << 1;
                    changed = true;
                    break;
                }
                offset += length;
            }
            assert!(changed);
            for hint in [false, true] {
                assert!(canonical(
                    Codec::Hevc,
                    NalFormat::LengthPrefixed(NalLengthSize::Four),
                    &data,
                    hint
                )
                .is_err());
            }
        }
    }
}
