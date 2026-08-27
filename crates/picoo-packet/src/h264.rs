//! Annex-B / AVCC parameter-set helpers — REQ-PICOO-PROTOCOL-005.

/// Split Annex-B byte stream into NAL units (start codes stripped).
pub fn split_annex_b_nals(data: &[u8]) -> Vec<&[u8]> {
    fn find_start(data: &[u8], from: usize) -> Option<(usize, usize)> {
        // (start_code_index, nal_payload_start)
        let mut i = from;
        while i + 3 <= data.len() {
            if i + 4 <= data.len() && data[i..i + 4] == [0, 0, 0, 1] {
                return Some((i, i + 4));
            }
            if data[i..i + 3] == [0, 0, 1] {
                return Some((i, i + 3));
            }
            i += 1;
        }
        None
    }

    let mut nals = Vec::new();
    let mut from = 0usize;
    while let Some((_, payload_start)) = find_start(data, from) {
        let next = find_start(data, payload_start)
            .map(|(pos, _)| pos)
            .unwrap_or(data.len());
        if next > payload_start {
            nals.push(&data[payload_start..next]);
        }
        from = next;
    }
    nals
}

/// Build Annex-B blob containing SPS then PPS (with start codes).
pub fn annex_b_parameter_sets(sps: &[u8], pps: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + sps.len() + pps.len());
    out.extend_from_slice(&[0, 0, 0, 1]);
    out.extend_from_slice(sps);
    out.extend_from_slice(&[0, 0, 0, 1]);
    out.extend_from_slice(pps);
    out
}

/// True when `data` is a complete 4-byte length-prefixed (AVCC-style) access unit.
///
/// Android MediaCodec H.264 elementary buffers commonly use this layout.
/// Note: NAL payloads may contain `00 00 01` emulation patterns, so we do **not**
/// reject based on interior start-code-like bytes; only a leading Annex-B start
/// code forces the Annex-B path.
pub fn is_length_prefixed_access_unit(data: &[u8]) -> bool {
    if data.len() < 5 {
        return false;
    }
    // Leading Annex-B start code → not length-prefixed.
    if data.starts_with(&[0, 0, 0, 1]) || data.starts_with(&[0, 0, 1]) {
        return false;
    }
    let mut i = 0usize;
    let mut nal_count = 0usize;
    while i + 4 <= data.len() {
        let len = u32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]) as usize;
        if len == 0 || i + 4 + len > data.len() {
            return false;
        }
        let nal_type = data[i + 4] & 0x1f;
        // Reject clearly non-VCL/non-parameter NAL types for elementary AUs.
        if nal_type == 0 || nal_type > 12 {
            return false;
        }
        i += 4 + len;
        nal_count += 1;
    }
    nal_count >= 1 && i == data.len()
}

/// Convert a length-prefixed AU into Annex-B (4-byte start codes). Returns `None` if not AVCC.
pub fn length_prefixed_to_annex_b(data: &[u8]) -> Option<Vec<u8>> {
    if !is_length_prefixed_access_unit(data) {
        return None;
    }
    let mut out = Vec::with_capacity(data.len() + 16);
    let mut i = 0usize;
    while i + 4 <= data.len() {
        let len = u32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]) as usize;
        i += 4;
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(&data[i..i + len]);
        i += len;
    }
    Some(out)
}

/// Normalize an access unit to Annex-B for soft/hardware decoders.
///
/// Passes Annex-B through unchanged; converts complete length-prefixed AUs.
pub fn access_unit_to_annex_b(data: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    if let Some(converted) = length_prefixed_to_annex_b(data) {
        std::borrow::Cow::Owned(converted)
    } else {
        std::borrow::Cow::Borrowed(data)
    }
}

/// Extract SPS (type 7) and PPS (type 8) from Annex-B or AVCC `csd-0` style blobs.
///
/// Returns NAL payloads without start codes.
pub fn extract_sps_pps(data: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    if data.is_empty() {
        return None;
    }

    if let Some(pair) = extract_avcc_parameter_sets(data) {
        return Some(pair);
    }

    let annex_b = data.windows(3).any(|w| w == [0, 0, 1]);
    if !annex_b {
        return None;
    }

    let mut sps: Option<Vec<u8>> = None;
    let mut pps: Option<Vec<u8>> = None;
    for nal in split_annex_b_nals(data) {
        if nal.is_empty() {
            continue;
        }
        match nal[0] & 0x1f {
            7 => sps = Some(nal.to_vec()),
            8 => pps = Some(nal.to_vec()),
            _ => {}
        }
    }
    Some((sps?, pps?))
}

/// Parse AVCDecoderConfigurationRecord-like layout used by MediaFormat `csd-0`.
fn extract_avcc_parameter_sets(data: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    // Minimal AVCC: configurationVersion(1)=1, profile(3), lengthSizeMinusOne(1),
    // numOfSPS(1)&0x1F, then SPS length(2)+SPS, numOfPPS(1), PPS length(2)+PPS.
    if data.len() < 7 || data[0] != 1 {
        return None;
    }
    let mut i = 5usize;
    let num_sps = (data[i] & 0x1f) as usize;
    i += 1;
    let mut sps = None;
    for _ in 0..num_sps {
        if i + 2 > data.len() {
            return None;
        }
        let len = u16::from_be_bytes([data[i], data[i + 1]]) as usize;
        i += 2;
        if i + len > data.len() || len == 0 {
            return None;
        }
        if sps.is_none() {
            sps = Some(data[i..i + len].to_vec());
        }
        i += len;
    }
    if i >= data.len() {
        return None;
    }
    let num_pps = data[i] as usize;
    i += 1;
    let mut pps = None;
    for _ in 0..num_pps {
        if i + 2 > data.len() {
            return None;
        }
        let len = u16::from_be_bytes([data[i], data[i + 1]]) as usize;
        i += 2;
        if i + len > data.len() || len == 0 {
            return None;
        }
        if pps.is_none() {
            pps = Some(data[i..i + len].to_vec());
        }
        i += len;
    }
    Some((sps?, pps?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_sps_pps_from_annex_b() {
        let sps = [0x67u8, 0x42, 0x00, 0x0a];
        let pps = [0x68u8, 0xce, 0x3c, 0x80];
        let mut annex = Vec::new();
        annex.extend_from_slice(&[0, 0, 0, 1]);
        annex.extend_from_slice(&sps);
        annex.extend_from_slice(&[0, 0, 0, 1]);
        annex.extend_from_slice(&pps);

        let (got_sps, got_pps) = extract_sps_pps(&annex).expect("pair");
        assert_eq!(got_sps, sps);
        assert_eq!(got_pps, pps);
    }

    #[test]
    fn extracts_sps_pps_from_avcc() {
        let sps = [0x67u8, 0x42, 0x00, 0x0a];
        let pps = [0x68u8, 0xce, 0x3c, 0x80];
        let mut avcc = vec![
            1,    // version
            0x42, // profile
            0x00, // compat
            0x0a, // level
            0xff, // lengthSizeMinusOne
            0xe1, // numSPS = 1
            0x00, 0x04, // sps len
        ];
        avcc.extend_from_slice(&sps);
        avcc.push(1); // numPPS
        avcc.extend_from_slice(&[0x00, 0x04]);
        avcc.extend_from_slice(&pps);

        let (got_sps, got_pps) = extract_sps_pps(&avcc).expect("pair");
        assert_eq!(got_sps, sps);
        assert_eq!(got_pps, pps);
    }

    #[test]
    fn annex_b_parameter_sets_round_trip() {
        let sps = [0x67u8, 0x42];
        let pps = [0x68u8, 0xce];
        let blob = annex_b_parameter_sets(&sps, &pps);
        let (got_sps, got_pps) = extract_sps_pps(&blob).expect("pair");
        assert_eq!(got_sps, sps);
        assert_eq!(got_pps, pps);
    }

    #[test]
    fn length_prefixed_au_converts_to_annex_b() {
        let nal = [0x65u8, 0x88, 0x84, 0x00]; // IDR slice header-ish
        let mut avcc = Vec::new();
        avcc.extend_from_slice(&(nal.len() as u32).to_be_bytes());
        avcc.extend_from_slice(&nal);
        assert!(is_length_prefixed_access_unit(&avcc));
        let annex = length_prefixed_to_annex_b(&avcc).expect("convert");
        assert_eq!(&annex[..4], &[0, 0, 0, 1]);
        assert_eq!(&annex[4..], &nal);
        assert!(matches!(
            access_unit_to_annex_b(&avcc),
            std::borrow::Cow::Owned(_)
        ));
    }

    #[test]
    fn annex_b_au_passes_through_normalize() {
        let annex = [0u8, 0, 0, 1, 0x65, 0x00];
        assert!(!is_length_prefixed_access_unit(&annex));
        assert!(matches!(
            access_unit_to_annex_b(&annex),
            std::borrow::Cow::Borrowed(_)
        ));
    }
}
