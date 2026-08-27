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
}
