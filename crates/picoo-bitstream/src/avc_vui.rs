//! Admit explicit BT.709 limited VUI on Android AVC SPS — REQ-PICOO-MEDIA-086.
//!
//! Protocol mapping still refuses unknown colour. This adapter only writes VUI
//! after the platform encoder has already declared BT.709 limited SDR.

use crate::{BitstreamError, VideoColorFacts, VideoSpsFacts};
use h264_reader::rbsp::decode_nal;

const BT709: VideoColorFacts = VideoColorFacts {
    full_range: false,
    primaries: 1,
    transfer: 1,
    matrix: 1,
};

pub(crate) fn ensure_bt709_limited(nal: &[u8]) -> Result<Vec<u8>, BitstreamError> {
    let facts = VideoSpsFacts::parse_avc(nal)?;
    match facts.color {
        Some(color) if color == BT709 => return Ok(nal.to_vec()),
        Some(color)
            if color.full_range
                || !matches!((color.primaries, color.transfer, color.matrix), (2, 2, 2)) =>
        {
            return Err(BitstreamError::Unsupported(
                "source is not BT.709 limited or unspecified SDR",
            ));
        }
        _ => {}
    }
    let header = *nal.first().ok_or(BitstreamError::Truncated)?;
    let rbsp = decode_nal(nal).map_err(|_| BitstreamError::Malformed("invalid AVC SPS RBSP"))?;
    let mut bits = Bits::from_rbsp(&rbsp);
    let vui_flag = skip_sps_to_vui_flag(&mut bits)?;
    let rewritten = if bits.read_at(vui_flag)? {
        rewrite_existing_vui(&bits, vui_flag)?
    } else {
        insert_minimal_vui(&bits, vui_flag)
    };
    let encoded = encode_nal(header, &rewritten);
    let admitted = VideoSpsFacts::parse_avc(&encoded)?;
    if admitted.color != Some(BT709)
        || (
            admitted.coded_width,
            admitted.coded_height,
            admitted.visible_x,
            admitted.visible_y,
            admitted.visible_width,
            admitted.visible_height,
            admitted.pixel_aspect_ratio,
            admitted.chroma_location,
        ) != (
            facts.coded_width,
            facts.coded_height,
            facts.visible_x,
            facts.visible_y,
            facts.visible_width,
            facts.visible_height,
            facts.pixel_aspect_ratio,
            facts.chroma_location,
        )
    {
        return Err(BitstreamError::Malformed(
            "rewritten SPS changed source geometry",
        ));
    }
    Ok(encoded)
}

fn rewrite_existing_vui(bits: &Bits, vui_flag: usize) -> Result<Vec<u8>, BitstreamError> {
    let mut cursor = vui_flag + 1;
    skip_aspect_ratio(bits, &mut cursor)?;
    skip_overscan(bits, &mut cursor)?;
    let video_signal_flag = cursor;
    cursor += 1;
    if bits.read_at(video_signal_flag)? {
        cursor += 3;
        if bits.read_at(cursor)? {
            return Err(BitstreamError::Unsupported("full-range AVC source"));
        }
        cursor += 1;
        if bits.read_at(cursor)? {
            cursor += 1;
            let mut patched = bits.clone();
            patched.write_u8(cursor, 1)?;
            patched.write_u8(cursor + 8, 1)?;
            patched.write_u8(cursor + 16, 1)?;
            return Ok(patched.finish_from(patched.stop_bit()?));
        }
        let mut out = bits.prefix(cursor);
        out.push(true);
        push_u8(&mut out, 1);
        push_u8(&mut out, 1);
        push_u8(&mut out, 1);
        out.extend_from_slice(&bits.bits[cursor + 1..bits.stop_bit()?]);
        return Ok(Bits { bits: out, pos: 0 }.finish_from(usize::MAX));
    }
    let mut out = bits.prefix(video_signal_flag);
    out.push(true);
    push_video_signal(&mut out);
    out.extend_from_slice(&bits.bits[video_signal_flag + 1..bits.stop_bit()?]);
    Ok(Bits { bits: out, pos: 0 }.finish_from(usize::MAX))
}

fn insert_minimal_vui(bits: &Bits, vui_flag: usize) -> Vec<u8> {
    let mut out = bits.prefix(vui_flag);
    out.push(true);
    out.push(false);
    out.push(false);
    out.push(true);
    push_video_signal(&mut out);
    out.push(false);
    out.push(false);
    out.push(false);
    out.push(false);
    out.push(false);
    out.push(false);
    Bits { bits: out, pos: 0 }.finish_from(usize::MAX)
}

fn push_video_signal(out: &mut Vec<bool>) {
    push_bits(out, 5, 3);
    out.push(false);
    out.push(true);
    push_u8(out, 1);
    push_u8(out, 1);
    push_u8(out, 1);
}

fn skip_sps_to_vui_flag(bits: &mut Bits) -> Result<usize, BitstreamError> {
    let profile = bits.read(8)? as u8;
    bits.skip(16)?;
    let _ = bits.read_ue()?;
    if matches!(profile, 100 | 110 | 122 | 244 | 44 | 83 | 86) {
        if bits.read_ue()? != 1 {
            return Err(BitstreamError::Unsupported("AVC requires 8-bit 4:2:0"));
        }
        let _ = bits.read_ue()?;
        let _ = bits.read_ue()?;
        bits.skip(1)?;
        if bits.read_bit()? {
            return Err(BitstreamError::Unsupported("AVC scaling matrix"));
        }
    }
    let _ = bits.read_ue()?;
    match bits.read_ue()? {
        0 => {
            let _ = bits.read_ue()?;
        }
        1 => {
            bits.skip(1)?;
            let _ = bits.read_se()?;
            let _ = bits.read_se()?;
            let cycle = bits.read_ue()?;
            if cycle > 255 {
                return Err(BitstreamError::Limit);
            }
            for _ in 0..cycle {
                let _ = bits.read_se()?;
            }
        }
        2 => {}
        _ => return Err(BitstreamError::Unsupported("AVC pic_order_cnt_type")),
    }
    let _ = bits.read_ue()?;
    bits.skip(1)?;
    let _ = bits.read_ue()?;
    let _ = bits.read_ue()?;
    if !bits.read_bit()? {
        return Err(BitstreamError::Unsupported("interlaced AVC"));
    }
    bits.skip(1)?;
    if bits.read_bit()? {
        let _ = bits.read_ue()?;
        let _ = bits.read_ue()?;
        let _ = bits.read_ue()?;
        let _ = bits.read_ue()?;
    }
    Ok(bits.pos)
}

fn skip_aspect_ratio(bits: &Bits, cursor: &mut usize) -> Result<(), BitstreamError> {
    if bits.read_at(*cursor)? {
        *cursor += 1;
        let idc = bits.read_n(*cursor, 8)?;
        *cursor += 8;
        if idc == 255 {
            *cursor += 32;
        }
    } else {
        *cursor += 1;
    }
    Ok(())
}

fn skip_overscan(bits: &Bits, cursor: &mut usize) -> Result<(), BitstreamError> {
    if bits.read_at(*cursor)? {
        *cursor += 2;
    } else {
        *cursor += 1;
    }
    Ok(())
}

fn encode_nal(header: u8, rbsp: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rbsp.len() + rbsp.len() / 2 + 1);
    out.push(header);
    let mut zeros = 0u8;
    for &byte in rbsp {
        if zeros >= 2 && byte <= 3 {
            out.push(3);
            zeros = 0;
        }
        out.push(byte);
        zeros = if byte == 0 { zeros + 1 } else { 0 };
    }
    out
}

fn push_u8(out: &mut Vec<bool>, value: u8) {
    push_bits(out, u64::from(value), 8);
}

fn push_bits(out: &mut Vec<bool>, value: u64, width: u32) {
    for shift in (0..width).rev() {
        out.push(((value >> shift) & 1) == 1);
    }
}

#[derive(Clone)]
struct Bits {
    bits: Vec<bool>,
    pos: usize,
}

impl Bits {
    fn from_rbsp(rbsp: &[u8]) -> Self {
        Self {
            bits: rbsp
                .iter()
                .flat_map(|byte| (0..8).rev().map(move |shift| ((byte >> shift) & 1) == 1))
                .collect(),
            pos: 0,
        }
    }

    fn prefix(&self, end: usize) -> Vec<bool> {
        self.bits[..end].to_vec()
    }

    fn read_bit(&mut self) -> Result<bool, BitstreamError> {
        let bit = self.read_at(self.pos)?;
        self.pos += 1;
        Ok(bit)
    }

    fn read_at(&self, index: usize) -> Result<bool, BitstreamError> {
        self.bits
            .get(index)
            .copied()
            .ok_or(BitstreamError::Truncated)
    }

    fn read(&mut self, width: u32) -> Result<u64, BitstreamError> {
        let value = self.read_n(self.pos, width)?;
        self.pos += width as usize;
        Ok(value)
    }

    fn read_n(&self, start: usize, width: u32) -> Result<u64, BitstreamError> {
        let mut value = 0u64;
        for offset in 0..width as usize {
            value = (value << 1) | u64::from(self.read_at(start + offset)?);
        }
        Ok(value)
    }

    fn skip(&mut self, width: u32) -> Result<(), BitstreamError> {
        let _ = self.read(width)?;
        Ok(())
    }

    fn read_ue(&mut self) -> Result<u32, BitstreamError> {
        let mut leading = 0u32;
        while !self.read_bit()? {
            leading += 1;
            if leading > 31 {
                return Err(BitstreamError::Limit);
            }
        }
        if leading == 0 {
            return Ok(0);
        }
        let suffix = self.read(leading)?;
        Ok(((1u64 << leading) - 1 + suffix) as u32)
    }

    fn read_se(&mut self) -> Result<i32, BitstreamError> {
        let ue = self.read_ue()?;
        let half = (ue >> 1) as i32;
        Ok(if ue & 1 == 0 { -half } else { half + 1 })
    }

    fn write_u8(&mut self, start: usize, value: u8) -> Result<(), BitstreamError> {
        if start + 8 > self.bits.len() {
            return Err(BitstreamError::Truncated);
        }
        for (offset, shift) in (0..8).rev().enumerate() {
            self.bits[start + offset] = ((value >> shift) & 1) == 1;
        }
        Ok(())
    }

    fn stop_bit(&self) -> Result<usize, BitstreamError> {
        self.bits
            .iter()
            .rposition(|bit| *bit)
            .ok_or(BitstreamError::Malformed("missing RBSP trailing bit"))
    }

    fn finish_from(&self, stop: usize) -> Vec<u8> {
        let payload = if stop == usize::MAX {
            let mut bits = self.bits.clone();
            bits.push(true);
            bits
        } else {
            let mut bits = self.bits[..stop].to_vec();
            bits.push(true);
            bits
        };
        let mut bytes = vec![0u8; payload.len().div_ceil(8)];
        for (index, bit) in payload.iter().enumerate() {
            if *bit {
                bytes[index / 8] |= 1 << (7 - (index % 8));
            }
        }
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{avc::extract_sps_pps, CodecConfiguration};

    fn fixture_sps() -> Vec<u8> {
        extract_sps_pps(include_bytes!(
            "../../picoo-testkit/fixtures/avc-64x64-bt709-idr.h264"
        ))
        .unwrap()
        .0
    }

    fn strip_vui(nal: &[u8]) -> Vec<u8> {
        let header = nal[0];
        let rbsp = decode_nal(nal).unwrap();
        let mut bits = Bits::from_rbsp(&rbsp);
        let vui_flag = skip_sps_to_vui_flag(&mut bits).unwrap();
        insert_minimal_vui_inverse(&bits, vui_flag, header)
    }

    fn insert_minimal_vui_inverse(bits: &Bits, vui_flag: usize, header: u8) -> Vec<u8> {
        let mut out = bits.prefix(vui_flag);
        out.push(false);
        encode_nal(header, &Bits { bits: out, pos: 0 }.finish_from(usize::MAX))
    }

    #[test]
    fn admitted_bt709_sps_is_left_unchanged() {
        let sps = fixture_sps();
        assert_eq!(ensure_bt709_limited(&sps).unwrap(), sps);
    }

    #[test]
    fn missing_vui_becomes_explicit_bt709_limited() {
        let stripped = strip_vui(&fixture_sps());
        assert_eq!(VideoSpsFacts::parse_avc(&stripped).unwrap().color, None);
        let admitted = ensure_bt709_limited(&stripped).unwrap();
        assert_eq!(
            VideoSpsFacts::parse_avc(&admitted).unwrap().color,
            Some(BT709)
        );
        assert_ne!(admitted, stripped);
    }

    #[test]
    fn rewritten_record_and_in_band_sps_share_one_identity() {
        let (sps, pps) = extract_sps_pps(include_bytes!(
            "../../picoo-testkit/fixtures/avc-64x64-bt709-idr.h264"
        ))
        .unwrap();
        let stripped = strip_vui(&sps);
        let original = CodecConfiguration::from_avc_parameter_sets(&stripped, &pps).unwrap();
        let admitted = original.clone().with_explicit_bt709_sdr().unwrap();
        let mut without_vui = Vec::new();
        for nal in crate::avc::split_annex_b_nals(include_bytes!(
            "../../picoo-testkit/fixtures/avc-64x64-bt709-idr.h264"
        )) {
            without_vui.extend_from_slice(&[0, 0, 0, 1]);
            if nal[0] & 31 == 7 {
                without_vui.extend_from_slice(&stripped);
            } else {
                without_vui.extend_from_slice(nal);
            }
        }
        let aligned = admitted.align_access_unit(&without_vui).unwrap();
        let picture =
            crate::AccessUnit::parse(crate::Codec::Avc, crate::NalFormat::AnnexB, &aligned)
                .unwrap();
        admitted.validate_parameter_sets(&picture).unwrap();
    }

    #[test]
    fn xiaomi_1080p_csd_already_declares_bt709_and_1088_storage() {
        let csd = hex_literal("000000016764002aacb403c0113f2cd40404041b4284d40000000168ee06f2c0");
        let original = CodecConfiguration::from_avc_annex_b(&csd).expect("parse Xiaomi CSD");
        let before = original.source_facts().expect("Xiaomi SPS facts");
        assert_eq!(before.color, Some(BT709), "{before:?}");
        assert_eq!(
            (
                before.coded_width,
                before.coded_height,
                before.visible_width,
                before.visible_height
            ),
            (1920, 1088, 1920, 1080),
            "{before:?}"
        );
        let admitted = original
            .clone()
            .with_explicit_bt709_sdr()
            .expect("rewrite Xiaomi VUI");
        let after = admitted.source_facts().expect("admitted Xiaomi facts");
        assert_eq!(after.color, Some(BT709));
        assert_eq!(
            (
                after.coded_width,
                after.coded_height,
                after.visible_width,
                after.visible_height
            ),
            (1920, 1088, 1920, 1080)
        );
    }

    fn hex_literal(hex: &str) -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn opaque_session_payload_is_left_unchanged() {
        let (sps, pps) = extract_sps_pps(include_bytes!(
            "../../picoo-testkit/fixtures/avc-64x64-bt709-idr.h264"
        ))
        .unwrap();
        let admitted = CodecConfiguration::from_avc_parameter_sets(&sps, &pps).unwrap();
        let payload = b"complete-native-idr";
        assert_eq!(admitted.align_access_unit(payload).unwrap(), payload);
    }

    #[test]
    fn unspecified_cicp_is_replaced_without_changing_geometry() {
        let original = VideoSpsFacts::parse_avc(&fixture_sps()).unwrap();
        let stripped = strip_vui(&fixture_sps());
        let admitted = ensure_bt709_limited(&stripped).unwrap();
        let facts = VideoSpsFacts::parse_avc(&admitted).unwrap();
        assert_eq!(facts.color, Some(BT709));
        assert_eq!(
            (
                facts.coded_width,
                facts.coded_height,
                facts.visible_width,
                facts.visible_height
            ),
            (
                original.coded_width,
                original.coded_height,
                original.visible_width,
                original.visible_height
            )
        );
    }
}
