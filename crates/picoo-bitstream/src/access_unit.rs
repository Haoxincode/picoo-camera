//! Bounded codec-specific AU inspection — REQ-PICOO-BITSTREAM-002.
use crate::{BitstreamError, Codec};

pub const MAX_ACCESS_UNIT_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_NALS_PER_ACCESS_UNIT: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NalFormat {
    AnnexB,
    LengthPrefixed(NalLengthSize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NalLengthSize {
    One,
    Two,
    Four,
}
impl NalLengthSize {
    pub fn bytes(self) -> usize {
        match self {
            Self::One => 1,
            Self::Two => 2,
            Self::Four => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RandomAccessPoint {
    AvcIdr,
    HevcIdr,
    HevcCra,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PictureKind {
    RandomAccess(RandomAccessPoint),
    Trailing,
    HevcRasl,
    HevcRadl,
}

/// Header-level evidence only. Native codec admission still validates full slice syntax.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PictureInfo {
    pub kind: PictureKind,
    pub discardable: bool,
}

/// Borrowed NALs are tied to one complete, bounded input. Representation is explicit:
/// a length of one and an Annex-B start code are never guessed from the same prefix.
#[derive(Debug)]
pub struct AccessUnit<'a> {
    codec: Codec,
    nals: Vec<&'a [u8]>,
    picture: PictureInfo,
}
impl<'a> AccessUnit<'a> {
    pub fn parse(codec: Codec, format: NalFormat, data: &'a [u8]) -> Result<Self, BitstreamError> {
        let nals = split_nals(format, data)?;
        let picture = inspect_picture(codec, &nals)?;
        Ok(Self {
            codec,
            nals,
            picture,
        })
    }
    pub fn codec(&self) -> Codec {
        self.codec
    }
    pub fn nals(&self) -> &[&'a [u8]] {
        &self.nals
    }
    pub fn picture(&self) -> PictureInfo {
        self.picture
    }
    pub fn to_length_prefixed(&self) -> Result<Vec<u8>, BitstreamError> {
        write_nals(&self.nals, false)
    }
    pub fn to_annex_b(&self) -> Result<Vec<u8>, BitstreamError> {
        write_nals(&self.nals, true)
    }
}

pub(crate) fn write_nals(nals: &[&[u8]], annex_b: bool) -> Result<Vec<u8>, BitstreamError> {
    let length = nals
        .iter()
        .try_fold(0usize, |sum, nal| {
            sum.checked_add(4)?.checked_add(nal.len())
        })
        .ok_or(BitstreamError::Limit)?;
    if length > MAX_ACCESS_UNIT_BYTES {
        return Err(BitstreamError::Limit);
    }
    let mut out = Vec::with_capacity(length);
    for nal in nals {
        out.extend_from_slice(&if annex_b { 1u32 } else { nal.len() as u32 }.to_be_bytes());
        out.extend_from_slice(nal);
    }
    Ok(out)
}

pub fn split_nals(format: NalFormat, data: &[u8]) -> Result<Vec<&[u8]>, BitstreamError> {
    if data.is_empty() {
        return Err(BitstreamError::Malformed("empty AU"));
    }
    if data.len() > MAX_ACCESS_UNIT_BYTES {
        return Err(BitstreamError::Limit);
    }
    let mut nals = Vec::new();
    match format {
        NalFormat::LengthPrefixed(size) => {
            let mut remaining = data;
            while !remaining.is_empty() {
                let prefix = remaining
                    .get(..size.bytes())
                    .ok_or(BitstreamError::Truncated)?;
                let length = prefix
                    .iter()
                    .fold(0usize, |length, b| (length << 8) | usize::from(*b));
                remaining = &remaining[size.bytes()..];
                let nal = remaining.get(..length).ok_or(BitstreamError::Truncated)?;
                push_nal(&mut nals, nal)?;
                remaining = &remaining[length..];
            }
        }
        NalFormat::AnnexB => {
            let (start, mut payload) =
                start_code(data, 0).ok_or(BitstreamError::Malformed("missing Annex-B prefix"))?;
            if data[..start].iter().any(|byte| *byte != 0) {
                return Err(BitstreamError::Malformed("Annex-B leading garbage"));
            }
            loop {
                let next = start_code(data, payload);
                let end = next.map_or(data.len(), |(start, _)| start);
                // Annex-B trailing_zero_8bits are outside the NAL payload.
                let mut nal = &data[payload..end];
                while nal.last() == Some(&0) {
                    nal = &nal[..nal.len() - 1];
                }
                push_nal(&mut nals, nal)?;
                match next {
                    Some((_, next_payload)) => payload = next_payload,
                    None => break,
                }
            }
        }
    }
    Ok(nals)
}
fn push_nal<'a>(nals: &mut Vec<&'a [u8]>, nal: &'a [u8]) -> Result<(), BitstreamError> {
    if nal.is_empty() {
        return Err(BitstreamError::Malformed("empty NAL"));
    }
    if nals.len() == MAX_NALS_PER_ACCESS_UNIT {
        return Err(BitstreamError::Limit);
    }
    nals.push(nal);
    Ok(())
}
fn start_code(data: &[u8], from: usize) -> Option<(usize, usize)> {
    let relative = data
        .get(from..)?
        .windows(3)
        .position(|bytes| bytes == [0, 0, 1])?;
    let end = from + relative + 3;
    let start = if relative > 0 && data[from + relative - 1] == 0 {
        from + relative - 1
    } else {
        from + relative
    };
    Some((start, end))
}

/// Fixed header validation for the supported single-layer AVC/HEVC contract.
pub(crate) fn nal_type(codec: Codec, nal: &[u8]) -> Result<u8, BitstreamError> {
    let first = *nal.first().ok_or(BitstreamError::Truncated)?;
    if first & 0x80 != 0 {
        return Err(BitstreamError::Malformed("forbidden_zero_bit"));
    }
    match codec {
        Codec::Avc => {
            let kind = first & 31;
            if !matches!(kind, 1 | 5..=12) {
                return Err(BitstreamError::Unsupported("AVC NAL type"));
            }
            if matches!(kind, 5 | 7 | 8) && first & 0x60 == 0 {
                return Err(BitstreamError::Malformed(
                    "AVC reference NAL without nal_ref_idc",
                ));
            }
            Ok(kind)
        }
        Codec::Hevc => {
            let second = *nal.get(1).ok_or(BitstreamError::Truncated)?;
            let kind = (first >> 1) & 63;
            if (first & 1) != 0 || second >> 3 != 0 {
                return Err(BitstreamError::Unsupported("HEVC multilayer"));
            }
            let temporal = second & 7;
            if temporal == 0 || (matches!(kind, 16..=23 | 32 | 33 | 36 | 37) && temporal != 1) {
                return Err(BitstreamError::Malformed("HEVC temporal id"));
            }
            if matches!(kind, 2..=5) && temporal == 1 {
                return Err(BitstreamError::Malformed("HEVC temporal switching id"));
            }
            if !matches!(kind, 0..=9 | 19..=21 | 32..=40) {
                return Err(BitstreamError::Unsupported("HEVC NAL type"));
            }
            Ok(kind)
        }
    }
}
fn inspect_picture(codec: Codec, nals: &[&[u8]]) -> Result<PictureInfo, BitstreamError> {
    let mut picture = None;
    let mut vcl_type = None;
    for nal in nals {
        let kind = nal_type(codec, nal)?;
        let vcl = match codec {
            Codec::Avc => matches!(kind, 1 | 5),
            Codec::Hevc => kind <= 31,
        };
        if !vcl {
            continue;
        }
        let first_slice = match codec {
            // first_mb_in_slice == 0 is the one-bit Exp-Golomb code '1'.
            Codec::Avc => *nal.get(1).ok_or(BitstreamError::Truncated)? & 0x80 != 0,
            Codec::Hevc => *nal.get(2).ok_or(BitstreamError::Truncated)? & 0x80 != 0,
        };
        if first_slice && picture.is_some() {
            return Err(BitstreamError::Unsupported("multiple pictures in AU"));
        }
        if !first_slice && picture.is_none() {
            return Err(BitstreamError::Malformed("missing first slice"));
        }
        if vcl_type.is_some_and(|previous| previous != kind) {
            return Err(BitstreamError::Unsupported("mixed VCL types"));
        }
        vcl_type = Some(kind);
        let current = PictureInfo {
            kind: match (codec, kind) {
                (Codec::Avc, 5) => PictureKind::RandomAccess(RandomAccessPoint::AvcIdr),
                (Codec::Hevc, 19 | 20) => PictureKind::RandomAccess(RandomAccessPoint::HevcIdr),
                (Codec::Hevc, 21) => PictureKind::RandomAccess(RandomAccessPoint::HevcCra),
                (Codec::Hevc, 8 | 9) => PictureKind::HevcRasl,
                (Codec::Hevc, 6 | 7) => PictureKind::HevcRadl,
                _ => PictureKind::Trailing,
            },
            discardable: match codec {
                Codec::Avc => nal[0] & 0x60 == 0,
                Codec::Hevc => kind <= 9 && kind % 2 == 0,
            },
        };
        if let Some(previous) = picture {
            if previous != current {
                return Err(BitstreamError::Unsupported(
                    "inconsistent picture reference status",
                ));
            }
        }
        picture = Some(current);
    }
    picture.ok_or(BitstreamError::Malformed("AU has no picture"))
}
