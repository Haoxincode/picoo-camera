//! PCP low-latency Reed-Solomon erasure protection for video fragments.
//!
//! Each group contains up to six data shards and two parity shards. Recovery
//! is local and immediate once enough shards arrive; it never waits for an RTT.

use std::ops::Range;
use std::sync::OnceLock;

use reed_solomon_erasure::galois_8::ReedSolomon;

pub const FEC_DATA_SHARDS: usize = 6;
pub const FEC_PARITY_SHARDS: usize = 2;
/// parity_index (u8) + last_data_shard_len (u16, network byte order).
pub const FEC_PARITY_PREFIX_SIZE: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FecParityShard {
    pub group_start: u16,
    pub parity_index: u8,
    pub last_data_len: u16,
    pub bytes: Vec<u8>,
}

/// Partition data fragments into balanced groups of at most six shards. This
/// avoids leaving a single unprotected shard at the end of an access unit.
pub fn fec_group_ranges(fragment_count: u16) -> Vec<Range<u16>> {
    if fragment_count == 0 {
        return Vec::new();
    }

    let fragment_count = usize::from(fragment_count);
    let group_count = fragment_count.div_ceil(FEC_DATA_SHARDS);
    let base_size = fragment_count / group_count;
    let larger_groups = fragment_count % group_count;
    let mut ranges = Vec::with_capacity(group_count);
    let mut start = 0_u16;
    for group_index in 0..group_count {
        let size = base_size + usize::from(group_index < larger_groups);
        let end = start + size as u16;
        ranges.push(start..end);
        start = end;
    }
    ranges
}

/// Return the balanced FEC group containing one data fragment without
/// allocating or walking every group in the access unit.
pub fn fec_group_for_fragment(fragment_count: u16, fragment_index: u16) -> Option<Range<u16>> {
    if fragment_count == 0 || fragment_index >= fragment_count {
        return None;
    }

    let fragment_count = usize::from(fragment_count);
    let fragment_index = usize::from(fragment_index);
    let group_count = fragment_count.div_ceil(FEC_DATA_SHARDS);
    let base_size = fragment_count / group_count;
    let larger_groups = fragment_count % group_count;
    let larger_size = base_size + 1;
    let larger_span = larger_groups * larger_size;
    let (group_index, start, size) = if fragment_index < larger_span {
        let group_index = fragment_index / larger_size;
        (group_index, group_index * larger_size, larger_size)
    } else {
        let group_index = larger_groups + (fragment_index - larger_span) / base_size;
        let start = larger_span + (group_index - larger_groups) * base_size;
        (group_index, start, base_size)
    };
    debug_assert!(group_index < group_count);
    Some(start as u16..(start + size) as u16)
}

pub fn make_fec_parity(group_start: u16, data: &[&[u8]]) -> Option<Vec<FecParityShard>> {
    make_fec_parity_count(group_start, data, FEC_PARITY_SHARDS)
}

/// Generate only the parity shards that will be transmitted. Parity index 0
/// remains byte-identical to the two-shard wire format used by Strong mode.
pub fn make_fec_parity_count(
    group_start: u16,
    data: &[&[u8]],
    parity_count: usize,
) -> Option<Vec<FecParityShard>> {
    let mut data_scratch = Vec::new();
    let mut parity_scratch = Vec::new();
    let last_data_len =
        encode_fec_parity_into(data, parity_count, &mut data_scratch, &mut parity_scratch)?;
    Some(
        parity_scratch
            .into_iter()
            .take(parity_count)
            .enumerate()
            .map(|(parity_index, bytes)| FecParityShard {
                group_start,
                parity_index: parity_index as u8,
                last_data_len,
                bytes,
            })
            .collect(),
    )
}

/// Encode into reusable padded-data and parity buffers.
pub fn encode_fec_parity_into(
    data: &[&[u8]],
    parity_count: usize,
    data_scratch: &mut Vec<Vec<u8>>,
    parity_scratch: &mut Vec<Vec<u8>>,
) -> Option<u16> {
    if !(2..=FEC_DATA_SHARDS).contains(&data.len()) {
        return None;
    }
    if !(1..=FEC_PARITY_SHARDS).contains(&parity_count) {
        return None;
    }
    let shard_len = data.iter().map(|shard| shard.len()).max()?;
    let last_data_len = u16::try_from(data.last()?.len()).ok()?;
    let codec = fec_codec(data.len(), parity_count)?;
    data_scratch.resize_with(data.len(), Vec::new);
    for (source, scratch) in data.iter().zip(data_scratch.iter_mut()) {
        scratch.resize(shard_len, 0);
        scratch.fill(0);
        scratch[..source.len()].copy_from_slice(source);
    }
    parity_scratch.resize_with(parity_count, Vec::new);
    for scratch in parity_scratch.iter_mut() {
        scratch.resize(shard_len, 0);
        scratch.fill(0);
    }
    codec
        .encode_sep(
            &data_scratch[..data.len()],
            &mut parity_scratch[..parity_count],
        )
        .ok()?;
    Some(last_data_len)
}

/// Reconstruct missing data shards in one group. Present data is copied into
/// padded shards; returned values retain `None` for unrecoverable fragments.
pub fn reconstruct_fec_group(
    data: &[Option<&[u8]>],
    parity: &[Option<&[u8]>],
    last_data_len: usize,
) -> Option<Vec<Option<Vec<u8>>>> {
    if !(2..=FEC_DATA_SHARDS).contains(&data.len()) || parity.len() != FEC_PARITY_SHARDS {
        return None;
    }
    let missing = data.iter().filter(|shard| shard.is_none()).count();
    let available_parity = parity.iter().filter(|shard| shard.is_some()).count();
    if missing == 0 || missing > available_parity {
        return None;
    }
    let shard_len = data
        .iter()
        .flatten()
        .map(|shard| shard.len())
        .chain(parity.iter().flatten().map(|shard| shard.len()))
        .max()?;
    if last_data_len > shard_len {
        return None;
    }

    let mut shards = data
        .iter()
        .chain(parity.iter())
        .map(|shard| {
            shard.map(|bytes| {
                let mut padded = vec![0_u8; shard_len];
                padded[..bytes.len()].copy_from_slice(bytes);
                padded
            })
        })
        .collect::<Vec<_>>();
    let codec = fec_codec(data.len(), FEC_PARITY_SHARDS)?;
    codec.reconstruct_data(&mut shards).ok()?;
    Some(
        shards
            .into_iter()
            .take(data.len())
            .enumerate()
            .map(|(index, shard)| {
                shard.map(|mut bytes| {
                    if index + 1 == data.len() {
                        bytes.truncate(last_data_len);
                    }
                    bytes
                })
            })
            .collect(),
    )
}

fn fec_codec(data_shards: usize, parity_shards: usize) -> Option<&'static ReedSolomon> {
    static CODECS: OnceLock<Vec<ReedSolomon>> = OnceLock::new();
    if !(2..=FEC_DATA_SHARDS).contains(&data_shards)
        || !(1..=FEC_PARITY_SHARDS).contains(&parity_shards)
    {
        return None;
    }
    CODECS
        .get_or_init(|| {
            (1..=FEC_PARITY_SHARDS)
                .flat_map(|parity| {
                    (2..=FEC_DATA_SHARDS).map(move |data| {
                        ReedSolomon::new(data, parity).expect("fixed PCP FEC parameters are valid")
                    })
                })
                .collect()
        })
        .get((parity_shards - 1) * (FEC_DATA_SHARDS - 1) + data_shards - 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_avoid_single_fragment_tail() {
        assert_eq!(fec_group_ranges(13), vec![0..5, 5..9, 9..13]);
        assert_eq!(fec_group_ranges(12), vec![0..6, 6..12]);
        assert_eq!(fec_group_ranges(7), vec![0..4, 4..7]);
        assert_eq!(fec_group_ranges(1), vec![0..1]);
    }

    #[test]
    fn direct_group_lookup_matches_balanced_ranges() {
        for fragment_count in 1..=1_024 {
            let ranges = fec_group_ranges(fragment_count);
            for fragment_index in 0..fragment_count {
                let expected = ranges
                    .iter()
                    .find(|range| range.contains(&fragment_index))
                    .cloned();
                assert_eq!(
                    fec_group_for_fragment(fragment_count, fragment_index),
                    expected
                );
            }
        }
        assert_eq!(fec_group_for_fragment(0, 0), None);
        assert_eq!(fec_group_for_fragment(7, 7), None);
    }

    #[test]
    fn two_missing_data_shards_are_reconstructed() {
        let owned = [b"aaaa".as_slice(), b"bbbb", b"cc"];
        let parity = make_fec_parity(0, &owned).expect("parity");
        let data = [None, Some(owned[1]), None];
        let parity_refs = [
            Some(parity[0].bytes.as_slice()),
            Some(parity[1].bytes.as_slice()),
        ];
        let rebuilt = reconstruct_fec_group(&data, &parity_refs, 2).expect("reconstruct");
        assert_eq!(rebuilt[0].as_deref(), Some(owned[0]));
        assert_eq!(rebuilt[1].as_deref(), Some(owned[1]));
        assert_eq!(rebuilt[2].as_deref(), Some(owned[2]));
    }

    #[test]
    fn one_transmitted_parity_recovers_one_missing_data_shard() {
        let owned = [b"aaaa".as_slice(), b"bbbb", b"cc"];
        let parity = make_fec_parity(0, &owned).expect("parity");
        let data = [Some(owned[0]), None, Some(owned[2])];
        let parity_refs = [Some(parity[0].bytes.as_slice()), None];
        let rebuilt = reconstruct_fec_group(&data, &parity_refs, 2).expect("reconstruct");
        assert_eq!(rebuilt[0].as_deref(), Some(owned[0]));
        assert_eq!(rebuilt[1].as_deref(), Some(owned[1]));
        assert_eq!(rebuilt[2].as_deref(), Some(owned[2]));
    }

    #[test]
    fn light_mode_computes_only_one_wire_compatible_parity() {
        let owned = [b"aaaa".as_slice(), b"bbbb", b"cc"];
        let strong = make_fec_parity_count(0, &owned, 2).expect("strong parity");
        let light = make_fec_parity_count(0, &owned, 1).expect("light parity");
        assert_eq!(light.len(), 1);
        assert_eq!(light[0].bytes, strong[0].bytes);
        assert_eq!(light[0].last_data_len, strong[0].last_data_len);
    }
}
