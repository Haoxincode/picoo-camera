use std::ops::Range;

use bytes::Bytes;
use picoo_protocol::{
    fec_group_for_fragment, fec_group_ranges, reconstruct_fec_group, FEC_PARITY_PREFIX_SIZE,
    FEC_PARITY_SHARDS, MAX_FEC_FRAGMENT_PAYLOAD,
};

use super::{PartialFrame, ReassemblyError};

#[derive(Debug)]
pub(super) struct FecGroupState {
    pub(super) range: Range<u16>,
    present_mask: u8,
    recovered_mask: u8,
    parity_shards: [Option<StoredParityShard>; FEC_PARITY_SHARDS],
}

#[derive(Debug)]
struct StoredParityShard {
    last_data_len: u16,
    bytes: Bytes,
}

pub(super) fn store_parity_shard(
    frame: &mut PartialFrame,
    group_start: u16,
    payload: Bytes,
) -> Result<usize, ReassemblyError> {
    let parity_index = payload[0];
    let last_data_len = u16::from_be_bytes([payload[1], payload[2]]);
    let parity_bytes = payload.slice(FEC_PARITY_PREFIX_SIZE..);
    let group_ix = frame
        .fragment_groups
        .get(usize::from(group_start))
        .copied()
        .filter(|group_ix| {
            let group = &frame.fec_groups[*group_ix];
            group.range.start == group_start && group.range.len() >= 2
        })
        .ok_or(ReassemblyError::InvalidFecParity)?;
    let slot = &mut frame.fec_groups[group_ix].parity_shards[usize::from(parity_index)];
    if slot.is_some() {
        return Err(ReassemblyError::DuplicateFragment);
    }
    *slot = Some(StoredParityShard {
        last_data_len,
        bytes: parity_bytes,
    });
    Ok(group_ix)
}

pub(super) fn validate_parity_shard(
    fragment_count: u16,
    group_start: u16,
    payload: &[u8],
) -> Result<(), ReassemblyError> {
    if payload.len() <= FEC_PARITY_PREFIX_SIZE {
        return Err(ReassemblyError::InvalidFecParity);
    }
    let parity_index = payload[0];
    if usize::from(parity_index) >= FEC_PARITY_SHARDS {
        return Err(ReassemblyError::InvalidFecParity);
    }
    let last_data_len = u16::from_be_bytes([payload[1], payload[2]]);
    let parity_bytes = &payload[FEC_PARITY_PREFIX_SIZE..];
    if last_data_len == 0
        || usize::from(last_data_len) > parity_bytes.len()
        || parity_bytes.len() > MAX_FEC_FRAGMENT_PAYLOAD
    {
        return Err(ReassemblyError::InvalidFecParity);
    }
    let valid_group = fec_group_for_fragment(fragment_count, group_start)
        .is_some_and(|group| group.start == group_start && group.len() >= 2);
    if !valid_group {
        return Err(ReassemblyError::InvalidFecParity);
    }
    Ok(())
}

pub(super) fn build_fec_state(fragment_count: u16) -> (Vec<FecGroupState>, Vec<usize>) {
    let ranges = fec_group_ranges(fragment_count);
    let mut fragment_groups = vec![0_usize; usize::from(fragment_count)];
    let groups = ranges
        .into_iter()
        .enumerate()
        .map(|(group_ix, range)| {
            for index in range.clone() {
                fragment_groups[usize::from(index)] = group_ix;
            }
            FecGroupState {
                range,
                present_mask: 0,
                recovered_mask: 0,
                parity_shards: std::array::from_fn(|_| None),
            }
        })
        .collect();
    (groups, fragment_groups)
}

pub(super) fn mark_data_present(frame: &mut PartialFrame, fragment_index: u16) -> usize {
    let group_ix = frame.fragment_groups[usize::from(fragment_index)];
    let offset = fragment_index - frame.fec_groups[group_ix].range.start;
    frame.fec_groups[group_ix].present_mask |= 1 << offset;
    group_ix
}

pub(super) fn is_recovered_fragment(frame: &PartialFrame, fragment_index: u16) -> bool {
    let Some(group_ix) = frame
        .fragment_groups
        .get(usize::from(fragment_index))
        .copied()
    else {
        return false;
    };
    let group = &frame.fec_groups[group_ix];
    let offset = fragment_index - group.range.start;
    group.recovered_mask & (1 << offset) != 0
}

/// Inspect only the group changed by the current packet. The bit masks make
/// the common no-parity path allocation-free; Reed-Solomon scratch is built
/// only once enough shards exist to attempt recovery.
pub(super) fn recover_affected_group(
    frame: &mut PartialFrame,
    group_ix: usize,
) -> (usize, bool, bool) {
    let group = &frame.fec_groups[group_ix];
    let group_len = group.range.len();
    if group_len < 2 {
        return (0, false, false);
    }

    let complete_mask = (1_u8 << group_len) - 1;
    let missing_mask = complete_mask & !group.present_mask;
    let missing_count = missing_mask.count_ones() as usize;
    if missing_count == 0 || missing_count > FEC_PARITY_SHARDS {
        return (0, true, false);
    }
    let available_parity = group
        .parity_shards
        .iter()
        .filter(|shard| shard.is_some())
        .count();
    if missing_count > available_parity {
        return (0, true, false);
    }
    let Some(last_data_len) = group
        .parity_shards
        .iter()
        .flatten()
        .map(|shard| shard.last_data_len)
        .next()
    else {
        return (0, true, false);
    };
    if group
        .parity_shards
        .iter()
        .flatten()
        .any(|shard| shard.last_data_len != last_data_len)
    {
        return (0, true, false);
    }

    let range = group.range.clone();
    let rebuilt = {
        let data = range
            .clone()
            .map(|index| frame.fragments.get(&index).map(Bytes::as_ref))
            .collect::<Vec<_>>();
        let parity = group
            .parity_shards
            .each_ref()
            .map(|shard| shard.as_ref().map(|shard| shard.bytes.as_ref()));
        reconstruct_fec_group(&data, &parity, usize::from(last_data_len))
    };
    let Some(rebuilt) = rebuilt else {
        return (0, true, true);
    };

    let mut recovered_total = 0usize;
    let mut recovered_mask = 0_u8;
    for (offset, shard) in rebuilt.into_iter().enumerate() {
        let bit = 1_u8 << offset;
        if missing_mask & bit == 0 {
            continue;
        }
        let Some(shard) = shard else {
            continue;
        };
        frame
            .fragments
            .insert(range.start + offset as u16, Bytes::from(shard));
        recovered_mask |= bit;
        recovered_total += 1;
    }
    let group = &mut frame.fec_groups[group_ix];
    group.present_mask |= recovered_mask;
    group.recovered_mask |= recovered_mask;
    if recovered_total > 0 {
        tracing::debug!(
            recovered_fragments = recovered_total,
            frame_fragments = frame.fragment_count,
            "reconstructed video fragments from FEC parity"
        );
    }
    (recovered_total, true, true)
}
