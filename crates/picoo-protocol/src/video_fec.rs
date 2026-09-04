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

pub fn make_fec_parity(group_start: u16, data: &[&[u8]]) -> Option<Vec<FecParityShard>> {
    if !(2..=FEC_DATA_SHARDS).contains(&data.len()) {
        return None;
    }
    let shard_len = data.iter().map(|shard| shard.len()).max()?;
    let last_data_len = u16::try_from(data.last()?.len()).ok()?;
    let codec = fec_codec(data.len())?;
    let mut shards = data
        .iter()
        .map(|data| {
            let mut shard = vec![0_u8; shard_len];
            shard[..data.len()].copy_from_slice(data);
            shard
        })
        .collect::<Vec<_>>();
    shards.extend((0..FEC_PARITY_SHARDS).map(|_| vec![0_u8; shard_len]));
    codec.encode(&mut shards).ok()?;
    Some(
        shards
            .into_iter()
            .skip(data.len())
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
    let codec = fec_codec(data.len())?;
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

fn fec_codec(data_shards: usize) -> Option<&'static ReedSolomon> {
    static CODECS: OnceLock<Vec<ReedSolomon>> = OnceLock::new();
    if !(2..=FEC_DATA_SHARDS).contains(&data_shards) {
        return None;
    }
    CODECS
        .get_or_init(|| {
            (2..=FEC_DATA_SHARDS)
                .map(|count| {
                    ReedSolomon::new(count, FEC_PARITY_SHARDS)
                        .expect("fixed PCP FEC parameters are valid")
                })
                .collect()
        })
        .get(data_shards - 2)
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
}
