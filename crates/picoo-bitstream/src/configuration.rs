//! Standard records are parsed by Scuffle; Picoo owns bounded product admission.
use crate::access_unit::nal_type;
use crate::{BitstreamError, Codec, NalLengthSize};
use bytes::Bytes;
use scuffle_h264::AVCDecoderConfigurationRecord;
use scuffle_h265::HEVCDecoderConfigurationRecord;
use std::io::Cursor;

const MAX_CONFIG_BYTES: usize = 64 * 1024;
const MAX_PARAMETER_SETS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodecConfiguration {
    codec: Codec,
    nal_length_size: NalLengthSize,
    profile_idc: u8,
    level_idc: u8,
    record: Bytes,
    vps: Vec<Bytes>,
    sps: Vec<Bytes>,
    pps: Vec<Bytes>,
}
impl CodecConfiguration {
    /// Parse a platform avcC/hvcC atom (without its MP4 box header).
    /// This validates storage and profile bounds, not complete parameter-set syntax.
    pub fn parse(codec: Codec, record: Bytes) -> Result<Self, BitstreamError> {
        if record.is_empty() || record.len() > MAX_CONFIG_BYTES {
            return Err(BitstreamError::Limit);
        }
        if record[0] != 1 {
            return Err(BitstreamError::Unsupported("configuration record version"));
        }
        preflight(codec, &record)?;
        let mut cursor = Cursor::new(record.clone());
        let mut config = Self {
            codec,
            nal_length_size: NalLengthSize::Four,
            profile_idc: 0,
            level_idc: 0,
            record: record.clone(),
            vps: Vec::new(),
            sps: Vec::new(),
            pps: Vec::new(),
        };
        match codec {
            Codec::Avc => {
                let avc = AVCDecoderConfigurationRecord::parse(&mut cursor)?;
                if avc.profile_indication != 100 {
                    return Err(BitstreamError::Unsupported(
                        "AVC profile is not High/Constrained High",
                    ));
                }
                if avc.extended_config.as_ref().is_some_and(|ext| {
                    ext.chroma_format_idc != 1
                        || ext.bit_depth_luma_minus8 != 0
                        || ext.bit_depth_chroma_minus8 != 0
                }) {
                    return Err(BitstreamError::Unsupported("AVC requires 8-bit 4:2:0"));
                }
                if avc.sps.iter().any(|sps| {
                    sps.len() < 4
                        || sps[1..4]
                            != [
                                avc.profile_indication,
                                avc.profile_compatibility,
                                avc.level_indication,
                            ]
                }) {
                    return Err(BitstreamError::Malformed("avcC header differs from SPS"));
                }
                config.profile_idc = avc.profile_indication;
                config.level_idc = avc.level_indication;
                config.nal_length_size = length_size(avc.length_size_minus_one)?;
                config.sps = avc.sps;
                config.pps = avc.pps;
            }
            Codec::Hevc => {
                let hevc = HEVCDecoderConfigurationRecord::demux(&mut cursor)?;
                if hevc.general_profile_space != 0
                    || hevc.general_profile_idc != 1
                    || hevc.chroma_format_idc != 1
                    || hevc.bit_depth_luma_minus8 != 0
                    || hevc.bit_depth_chroma_minus8 != 0
                {
                    return Err(BitstreamError::Unsupported(
                        "HEVC requires Main 8-bit 4:2:0",
                    ));
                }
                config.profile_idc = hevc.general_profile_idc;
                config.level_idc = hevc.general_level_idc;
                config.nal_length_size = length_size(hevc.length_size_minus_one)?;
                for array in hevc.arrays {
                    match u8::from(array.nal_unit_type) {
                        32 => config.vps.extend(array.nalus),
                        33 => config.sps.extend(array.nalus),
                        34 => config.pps.extend(array.nalus),
                        _ => {
                            return Err(BitstreamError::Unsupported(
                                "configuration contains non-parameter NAL",
                            ))
                        }
                    }
                }
            }
        }
        if cursor.position() != record.len() as u64 {
            return Err(BitstreamError::Malformed("trailing configuration bytes"));
        }
        if config.sps.is_empty()
            || config.pps.is_empty()
            || (codec == Codec::Hevc && config.vps.is_empty())
        {
            return Err(BitstreamError::Malformed("incomplete parameter sets"));
        }
        for (sets, expected) in [
            (&config.vps, 32),
            (&config.sps, if codec == Codec::Avc { 7 } else { 33 }),
            (&config.pps, if codec == Codec::Avc { 8 } else { 34 }),
        ] {
            for set in sets {
                if set.len() < 3 || nal_type(codec, set)? != expected {
                    return Err(BitstreamError::Malformed("parameter NAL type"));
                }
            }
        }
        Ok(config)
    }
    /// Convert the native AVC adapter's raw parameter NALs to standard avcC.
    /// Storage, NAL headers and profile are checked; this is not full SPS proof.
    pub fn from_avc_parameter_sets(sps: &[u8], pps: &[u8]) -> Result<Self, BitstreamError> {
        if sps.len().saturating_add(pps.len()).saturating_add(11) > MAX_CONFIG_BYTES {
            return Err(BitstreamError::Limit);
        }
        if sps.len() < 4
            || pps.len() < 3
            || nal_type(Codec::Avc, sps)? != 7
            || nal_type(Codec::Avc, pps)? != 8
        {
            return Err(BitstreamError::Malformed("raw AVC parameter sets required"));
        }
        let avc = AVCDecoderConfigurationRecord {
            configuration_version: 1,
            profile_indication: sps[1],
            profile_compatibility: sps[2],
            level_indication: sps[3],
            length_size_minus_one: 3,
            sps: vec![Bytes::copy_from_slice(sps)],
            pps: vec![Bytes::copy_from_slice(pps)],
            extended_config: None,
        };
        let mut record = Vec::with_capacity(avc.size() as usize);
        avc.build(&mut record)?;
        Self::parse(Codec::Avc, record.into())
    }

    /// Admit MediaCodec AVC codec-config bytes with explicit Annex B framing.
    /// REQ-PICOO-MEDIA-033: raw parameters leave this platform adaptation boundary.
    pub fn from_avc_annex_b(data: &[u8]) -> Result<Self, BitstreamError> {
        if data.len() > MAX_CONFIG_BYTES {
            return Err(BitstreamError::Limit);
        }
        let mut sps = None;
        let mut pps = None;
        for nal in crate::split_nals(crate::NalFormat::AnnexB, data)? {
            let target = match nal_type(Codec::Avc, nal)? {
                7 => &mut sps,
                8 => &mut pps,
                _ => {
                    return Err(BitstreamError::Malformed(
                        "AVC codec-config contains a non-parameter NAL",
                    ))
                }
            };
            if target.is_some_and(|previous| previous != nal) {
                return Err(BitstreamError::Unsupported(
                    "multiple distinct native AVC parameter sets",
                ));
            }
            *target = Some(nal);
        }
        Self::from_avc_parameter_sets(
            sps.ok_or(BitstreamError::Malformed("missing AVC SPS"))?,
            pps.ok_or(BitstreamError::Malformed("missing AVC PPS"))?,
        )
    }

    /// REQ-PICOO-BITSTREAM-005: a picture cannot replace committed parameter sets.
    /// Checks byte identity of every in-band VPS/SPS/PPS, including isolated updates.
    /// Native decoding remains responsible for full picture syntax and references.
    pub fn validate_parameter_sets(
        &self,
        picture: &crate::AccessUnit<'_>,
    ) -> Result<(), BitstreamError> {
        if picture.codec() != self.codec {
            return Err(BitstreamError::Malformed(
                "picture codec differs from configuration",
            ));
        }
        for nal in picture.nals() {
            let expected = match (self.codec, nal_type(self.codec, nal)?) {
                (Codec::Avc, 7) | (Codec::Hevc, 33) => Some(self.sps()),
                (Codec::Avc, 8) | (Codec::Hevc, 34) => Some(self.pps()),
                (Codec::Hevc, 32) => Some(self.vps()),
                _ => None,
            };
            if expected.is_some_and(|sets| !sets.iter().any(|set| set.as_ref() == *nal)) {
                return Err(BitstreamError::Malformed(
                    "in-band parameter set differs from configuration",
                ));
            }
        }
        Ok(())
    }

    /// All declared SPS must describe the same source before an owner commits it.
    /// Standard syntax remains in the shared bounded AVC/HEVC parsers.
    pub fn source_facts(&self) -> Result<crate::VideoSpsFacts, BitstreamError> {
        let parse = match self.codec {
            Codec::Avc => crate::VideoSpsFacts::parse_avc,
            Codec::Hevc => crate::VideoSpsFacts::parse_hevc,
        };
        let mut facts = None;
        for sps in &self.sps {
            let current = parse(sps)?;
            if facts.is_some_and(|previous| previous != current) {
                return Err(BitstreamError::Malformed("conflicting SPS source facts"));
            }
            facts = Some(current);
        }
        facts.ok_or(BitstreamError::Malformed("missing source SPS"))
    }

    /// Stream dimensions describe the visible image, including codec padding crop.
    pub fn validate_visible_size(&self, width: u32, height: u32) -> Result<(), BitstreamError> {
        let facts = self.source_facts()?;
        if (facts.visible_width, facts.visible_height) != (width, height) {
            return Err(BitstreamError::Malformed(
                "source dimensions differ from SPS",
            ));
        }
        Ok(())
    }

    pub fn profile_idc(&self) -> u8 {
        self.profile_idc
    }
    pub fn level_idc(&self) -> u8 {
        self.level_idc
    }

    pub fn codec(&self) -> Codec {
        self.codec
    }
    pub fn nal_length_size(&self) -> NalLengthSize {
        self.nal_length_size
    }
    pub fn record(&self) -> &Bytes {
        &self.record
    }
    pub fn vps(&self) -> &[Bytes] {
        &self.vps
    }
    pub fn sps(&self) -> &[Bytes] {
        &self.sps
    }
    pub fn pps(&self) -> &[Bytes] {
        &self.pps
    }
}
fn length_size(minus_one: u8) -> Result<NalLengthSize, BitstreamError> {
    match minus_one {
        0 => Ok(NalLengthSize::One),
        1 => Ok(NalLengthSize::Two),
        3 => Ok(NalLengthSize::Four),
        _ => Err(BitstreamError::Unsupported("NAL length size")),
    }
}

/// Bound array allocations before entering the general-purpose parser. This scan
/// does not interpret SPS syntax or duplicate the standard record implementation.
fn preflight(codec: Codec, data: &[u8]) -> Result<(), BitstreamError> {
    let mut reader = RecordBounds {
        data,
        offset: 0,
        count: 0,
    };
    match codec {
        Codec::Avc => {
            reader.skip(5)?;
            let count = reader.byte()? & 31;
            reader.sets(usize::from(count))?;
            let count = reader.byte()?;
            reader.sets(usize::from(count))?;
            if reader.offset < data.len() {
                reader.skip(3)?;
                if reader.byte()? != 0 {
                    return Err(BitstreamError::Unsupported("AVC SPS extensions"));
                }
            }
        }
        Codec::Hevc => {
            reader.skip(22)?;
            let arrays = reader.byte()?;
            if arrays > 3 {
                return Err(BitstreamError::Unsupported("HEVC configuration arrays"));
            }
            for _ in 0..arrays {
                let kind = reader.byte()? & 63;
                if !matches!(kind, 32..=34) {
                    return Err(BitstreamError::Unsupported("HEVC configuration NAL type"));
                }
                let count = reader.u16()?;
                reader.sets(usize::from(count))?;
            }
        }
    }
    if reader.offset != data.len() {
        return Err(BitstreamError::Malformed("configuration trailing bytes"));
    }
    Ok(())
}
struct RecordBounds<'a> {
    data: &'a [u8],
    offset: usize,
    count: usize,
}
impl RecordBounds<'_> {
    fn skip(&mut self, count: usize) -> Result<(), BitstreamError> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or(BitstreamError::Limit)?;
        if end > self.data.len() {
            return Err(BitstreamError::Truncated);
        }
        self.offset = end;
        Ok(())
    }
    fn byte(&mut self) -> Result<u8, BitstreamError> {
        let byte = *self
            .data
            .get(self.offset)
            .ok_or(BitstreamError::Truncated)?;
        self.offset += 1;
        Ok(byte)
    }
    fn u16(&mut self) -> Result<u16, BitstreamError> {
        Ok(u16::from_be_bytes([self.byte()?, self.byte()?]))
    }
    fn sets(&mut self, count: usize) -> Result<(), BitstreamError> {
        self.count = self.count.checked_add(count).ok_or(BitstreamError::Limit)?;
        if self.count > MAX_PARAMETER_SETS {
            return Err(BitstreamError::Limit);
        }
        for _ in 0..count {
            let length = self.u16()?;
            if length == 0 {
                return Err(BitstreamError::Malformed("empty parameter set"));
            }
            self.skip(usize::from(length))?;
        }
        Ok(())
    }
}
