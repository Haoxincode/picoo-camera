//! Video fragment reassembly — REQ-PICOO-PROTOCOL-004.

mod h264;

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use bytes::{Bytes, BytesMut};
pub use h264::{
    access_unit_contains_idr, access_unit_to_annex_b, annex_b_parameter_sets,
    annex_b_to_length_prefixed, extract_sps_pps, is_length_prefixed_access_unit,
    length_prefixed_to_annex_b, split_annex_b_nals,
};
use picoo_protocol::{
    fec_group_ranges, reconstruct_fec_group, VideoPacket, VideoPacketFlags, FEC_PARITY_PREFIX_SIZE,
    FEC_PARITY_SHARDS, MAX_FEC_FRAGMENT_PAYLOAD,
};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct FrameKey {
    stream_epoch: u32,
    frame_id: u64,
}

#[derive(Debug)]
struct PartialFrame {
    fragments: HashMap<u16, Bytes>,
    recovered_fragments: HashSet<u16>,
    parity_shards: HashMap<(u16, u8), StoredParityShard>,
    fragment_count: u16,
    flags: picoo_protocol::VideoPacketFlags,
    pts_us: u64,
    encoded_at_us: u64,
    first_fragment_at: Instant,
}

#[derive(Debug)]
struct StoredParityShard {
    last_data_len: u16,
    bytes: Bytes,
}

#[derive(Debug)]
struct PendingFrameGap {
    noticed_at: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssembledAccessUnit {
    pub data: Bytes,
    pub frame_id: u64,
    pub pts_us: u64,
    pub encoded_at_us: u64,
    pub keyframe: bool,
    pub discardable: bool,
    pub stream_epoch: u32,
    pub fragment_count: u16,
    /// Receiver-local time at which the first fragment entered reassembly.
    pub first_fragment_at: Instant,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ReassemblyError {
    #[error("fragment_count exceeds limit")]
    TooManyFragments,
    #[error("duplicate fragment index")]
    DuplicateFragment,
    #[error("epoch mismatch")]
    EpochMismatch,
    #[error("invalid FEC parity shard")]
    InvalidFecParity,
    #[error("fragments for one access unit carry inconsistent timeline metadata")]
    InconsistentFrameMetadata,
}

pub struct ReassemblyMap {
    max_frames: usize,
    max_fragments: u16,
    current_epoch: u32,
    expired_through_frame_id: Option<u64>,
    frames: HashMap<FrameKey, PartialFrame>,
    highest_observed_frame_id: Option<u64>,
    frame_gaps: HashMap<u64, PendingFrameGap>,
    rejected_frames: HashSet<FrameKey>,
    terminal_frames: HashSet<FrameKey>,
    drops: u64,
    partial_access_unit_drops: u64,
    whole_access_unit_gap_drops: u64,
    missing_fragments: u64,
    /// Total expected fragments for AUs whose complete/drop outcome is known.
    resolved_fragments: u64,
    /// Data fragments reconstructed from parity before their AU deadline.
    fec_recovered_fragments: u64,
    /// Set when a non-discardable AU is dropped and the prediction chain may be invalid.
    reference_loss_pending: bool,
    /// Set when a partial KEYFRAME is discarded (REQ-PICOO-SESSION-003).
    keyframe_loss_pending: bool,
}

impl ReassemblyMap {
    pub fn new(max_frames: usize, max_fragments: u16) -> Self {
        Self {
            max_frames: max_frames.max(1),
            max_fragments,
            current_epoch: 0,
            expired_through_frame_id: None,
            frames: HashMap::new(),
            highest_observed_frame_id: None,
            frame_gaps: HashMap::new(),
            rejected_frames: HashSet::new(),
            terminal_frames: HashSet::new(),
            drops: 0,
            partial_access_unit_drops: 0,
            whole_access_unit_gap_drops: 0,
            missing_fragments: 0,
            resolved_fragments: 0,
            fec_recovered_fragments: 0,
            reference_loss_pending: false,
            keyframe_loss_pending: false,
        }
    }

    pub fn drop_count(&self) -> u64 {
        self.drops
    }

    /// AUs for which at least one data fragment arrived, but the AU could not
    /// be completed before eviction/deadline.
    pub fn partial_access_unit_drop_count(&self) -> u64 {
        self.partial_access_unit_drops
    }

    /// AUs inferred solely from a discontinuity in the monotonic frame id.
    /// No fragment for these AUs reached Receiver before their deadline.
    pub fn whole_access_unit_gap_drop_count(&self) -> u64 {
        self.whole_access_unit_gap_drops
    }

    /// Fragments known to be absent when an incomplete AU is expired or evicted.
    ///
    /// This deliberately excludes an AU for which no fragment arrived at all;
    /// observing that case requires a transport-wide packet sequence.
    pub fn missing_fragment_count(&self) -> u64 {
        self.missing_fragments
    }

    /// Expected fragment count for all completed or confirmed-incomplete AUs.
    /// This advances atomically with each AU outcome so reporting windows never
    /// split a frame's received and missing fragments.
    pub fn resolved_fragment_count(&self) -> u64 {
        self.resolved_fragments
    }

    pub fn fec_recovered_fragment_count(&self) -> u64 {
        self.fec_recovered_fragments
    }

    /// Oldest Sender PTS that still has an unresolved reassembly entry.
    /// Jitter playout uses this to avoid emitting a newer complete AU while an
    /// older AU is still legitimately inside its reassembly deadline.
    pub fn oldest_pending_pts_us(&self) -> Option<u64> {
        self.frames.values().map(|frame| frame.pts_us).min()
    }

    /// Oldest unresolved frame id, including an AU for which no Datagram has
    /// arrived yet but whose id is bracketed by newer media.
    pub fn oldest_unresolved_frame_id(&self) -> Option<u64> {
        self.frames
            .keys()
            .map(|key| key.frame_id)
            .chain(self.frame_gaps.keys().copied())
            .min()
    }

    /// Discard pending media without classifying still-in-flight fragments as loss.
    /// A monotonic boundary prevents late tails from rebuilding abandoned AUs in
    /// this epoch even after the bounded terminal-frame cache rotates.
    pub fn clear_pending(&mut self) {
        let abandoned = self.frames.drain().map(|(key, _)| key).collect::<Vec<_>>();
        let latest_abandoned = abandoned
            .iter()
            .map(|key| key.frame_id)
            .chain(self.frame_gaps.keys().copied())
            .max();
        if let Some(latest) = latest_abandoned {
            self.expired_through_frame_id = Some(
                self.expired_through_frame_id
                    .map_or(latest, |expired| expired.max(latest)),
            );
        }
        self.frame_gaps.clear();
        for key in abandoned {
            self.remember_terminal(key);
        }
    }

    /// True if a keyframe was dropped since the last take (REQ-PICOO-SESSION-003).
    pub fn take_keyframe_loss(&mut self) -> bool {
        let pending = self.keyframe_loss_pending;
        self.keyframe_loss_pending = false;
        pending
    }

    /// True when a non-discardable AU was discarded since the last take.
    /// Receiver must stop feeding dependent delta AUs until a fresh IDR arrives.
    pub fn take_reference_chain_loss(&mut self) -> bool {
        let pending = self.reference_loss_pending;
        self.reference_loss_pending = false;
        if pending {
            self.keyframe_loss_pending = false;
        }
        pending
    }

    /// Discard incomplete access units whose first fragment exceeded the
    /// reassembly deadline. A monotonic frame boundary prevents late tail
    /// fragments from recreating an already-expired AU.
    pub fn expire_incomplete_older_than(&mut self, now: Instant, max_age: Duration) {
        let Some(deadline) = now.checked_sub(max_age) else {
            return;
        };
        let expired_partial_through = self
            .frames
            .iter()
            .filter(|(_, frame)| frame.first_fragment_at <= deadline)
            .map(|(key, _)| key.frame_id)
            .max();
        let expired_gap_through = self
            .frame_gaps
            .iter()
            .filter(|(_, gap)| gap.noticed_at <= deadline)
            .map(|(frame_id, _)| *frame_id)
            .max();
        let expired_through = match (expired_partial_through, expired_gap_through) {
            (Some(partial), Some(gap)) => Some(partial.max(gap)),
            (partial, gap) => partial.or(gap),
        };
        let Some(expired_through) = expired_through else {
            return;
        };
        // If a newer frame reached its deadline, older media is already past
        // the same playout horizon even when its first fragment arrived late.
        let expired = self
            .frames
            .keys()
            .filter(|key| key.frame_id <= expired_through)
            .copied()
            .collect::<Vec<_>>();
        for key in expired {
            if let Some(frame) = self.frames.remove(&key) {
                self.record_incomplete_drop(frame);
            }
        }
        let expired_gaps = self
            .frame_gaps
            .keys()
            .filter(|frame_id| **frame_id <= expired_through)
            .copied()
            .collect::<Vec<_>>();
        if !expired_gaps.is_empty() {
            tracing::warn!(
                first_missing_frame_id = expired_gaps.iter().min().copied(),
                last_missing_frame_id = expired_gaps.iter().max().copied(),
                missing_access_units = expired_gaps.len(),
                "dropping unresolved whole-access-unit frame-id gap"
            );
            self.reference_loss_pending = true;
            self.drops = self.drops.saturating_add(expired_gaps.len() as u64);
            self.whole_access_unit_gap_drops = self
                .whole_access_unit_gap_drops
                .saturating_add(expired_gaps.len() as u64);
            for frame_id in expired_gaps {
                self.frame_gaps.remove(&frame_id);
            }
        }
        self.expired_through_frame_id = Some(
            self.expired_through_frame_id
                .map_or(expired_through, |expired| expired.max(expired_through)),
        );
        self.rejected_frames
            .retain(|rejected| rejected.frame_id > expired_through);
    }

    pub fn ingest(
        &mut self,
        packet: VideoPacket,
    ) -> Result<Option<AssembledAccessUnit>, ReassemblyError> {
        if packet.stream_epoch < self.current_epoch {
            return Ok(None);
        }

        if packet.stream_epoch > self.current_epoch {
            self.reset_pending_for_epoch();
            self.current_epoch = packet.stream_epoch;
        }

        let key = FrameKey {
            stream_epoch: packet.stream_epoch,
            frame_id: packet.frame_id,
        };

        let inferred_first_fragment_at = self.observe_frame_id(packet.frame_id);

        if self
            .expired_through_frame_id
            .is_some_and(|expired| packet.frame_id <= expired)
            || self.terminal_frames.contains(&key)
        {
            return Ok(None);
        }

        if self.rejected_frames.contains(&key) {
            return Err(ReassemblyError::TooManyFragments);
        }
        if packet.fragment_count > self.max_fragments {
            if self.rejected_frames.len() >= self.max_frames.max(1) {
                self.drop_oldest_rejected();
            }
            if self.rejected_frames.insert(key) {
                self.drops += 1;
                if !packet.flags.contains(VideoPacketFlags::DISCARDABLE) {
                    self.reference_loss_pending = true;
                }
                if Self::is_keyframe(packet.flags) {
                    self.keyframe_loss_pending = true;
                }
            }
            return Err(ReassemblyError::TooManyFragments);
        }

        if self.frames.len() >= self.max_frames && !self.frames.contains_key(&key) {
            self.drop_oldest();
        }

        let packet_flags = packet.flags;
        let packet_pts = packet.pts_us;
        let packet_encoded_at = packet.encoded_at_us;
        let packet_epoch = packet.stream_epoch;
        if packet.flags.contains(VideoPacketFlags::FEC_PARITY) {
            validate_parity_shard(
                packet.fragment_count,
                packet.fragment_index,
                &packet.payload,
            )?;
        }

        let recovered_now = {
            let entry = self.frames.entry(key).or_insert_with(|| PartialFrame {
                fragment_count: packet.fragment_count,
                flags: packet_flags,
                pts_us: packet_pts,
                encoded_at_us: packet_encoded_at,
                fragments: HashMap::new(),
                recovered_fragments: HashSet::new(),
                parity_shards: HashMap::new(),
                first_fragment_at: inferred_first_fragment_at.unwrap_or_else(Instant::now),
            });

            if entry.fragment_count != packet.fragment_count {
                let frame = self.frames.remove(&key).expect("reassembly entry exists");
                self.record_incomplete_drop(frame);
                self.remember_terminal(key);
                return Ok(None);
            }
            let semantic_flags = VideoPacketFlags::KEYFRAME | VideoPacketFlags::DISCARDABLE;
            if entry.pts_us != packet_pts
                || entry.encoded_at_us != packet_encoded_at
                || entry.flags.intersection(semantic_flags)
                    != packet_flags.intersection(semantic_flags)
            {
                let frame = self.frames.remove(&key).expect("reassembly entry exists");
                self.record_incomplete_drop(frame);
                self.remember_terminal(key);
                return Err(ReassemblyError::InconsistentFrameMetadata);
            }

            if packet.flags.contains(VideoPacketFlags::FEC_PARITY) {
                store_parity_shard(entry, packet.fragment_index, packet.payload)?;
            } else {
                if entry.fragments.contains_key(&packet.fragment_index) {
                    // A systematic fragment may arrive after parity already
                    // reconstructed it. It is redundant, not a protocol fault.
                    if entry.recovered_fragments.contains(&packet.fragment_index) {
                        return Ok(None);
                    }
                    return Err(ReassemblyError::DuplicateFragment);
                }
                entry
                    .fragments
                    .insert(packet.fragment_index, packet.payload);
            }

            recover_available_groups(entry)
        };
        self.fec_recovered_fragments = self
            .fec_recovered_fragments
            .saturating_add(recovered_now as u64);

        let complete = self
            .frames
            .get(&key)
            .is_some_and(|entry| entry.fragments.len() as u16 == entry.fragment_count);
        if !complete {
            return Ok(None);
        }

        let entry = self
            .frames
            .remove(&key)
            .expect("complete reassembly entry exists");
        let mut assembled =
            BytesMut::with_capacity(entry.fragments.values().map(|p| p.len()).sum());
        for index in 0..entry.fragment_count {
            if let Some(chunk) = entry.fragments.get(&index) {
                assembled.extend_from_slice(chunk);
            } else {
                self.record_incomplete_drop(entry);
                self.remember_terminal(key);
                return Ok(None);
            }
        }

        let flags = entry.flags;
        let pts_us = entry.pts_us;
        let encoded_at_us = entry.encoded_at_us;
        let fragment_count = entry.fragment_count;
        let first_fragment_at = entry.first_fragment_at;
        self.remember_terminal(key);
        self.resolved_fragments = self
            .resolved_fragments
            .saturating_add(u64::from(fragment_count));
        Ok(Some(AssembledAccessUnit {
            data: assembled.freeze(),
            frame_id: key.frame_id,
            pts_us,
            encoded_at_us,
            keyframe: Self::is_keyframe(flags),
            discardable: flags.contains(VideoPacketFlags::DISCARDABLE),
            stream_epoch: packet_epoch,
            fragment_count,
            first_fragment_at,
        }))
    }

    pub fn is_keyframe(flags: VideoPacketFlags) -> bool {
        flags.contains(VideoPacketFlags::KEYFRAME)
    }

    fn drop_oldest(&mut self) {
        let oldest_non_keyframe = self
            .frames
            .iter()
            .filter(|(_, frame)| !Self::is_keyframe(frame.flags))
            .min_by_key(|(key, _)| key.frame_id)
            .map(|(key, _)| *key);
        let oldest = oldest_non_keyframe
            .or_else(|| self.frames.keys().min_by_key(|key| key.frame_id).copied());
        if let Some(key) = oldest {
            if let Some(frame) = self.frames.remove(&key) {
                self.record_incomplete_drop(frame);
            }
            self.remember_terminal(key);
        }
    }

    fn record_incomplete_drop(&mut self, frame: PartialFrame) {
        tracing::warn!(
            frame_fragments = frame.fragment_count,
            received_fragments = frame.fragments.len(),
            missing_fragments = frame
                .fragment_count
                .saturating_sub(frame.fragments.len() as u16),
            keyframe = Self::is_keyframe(frame.flags),
            age_ms = frame.first_fragment_at.elapsed().as_secs_f64() * 1_000.0,
            "dropping incomplete video access unit"
        );
        if !frame.flags.contains(VideoPacketFlags::DISCARDABLE) {
            self.reference_loss_pending = true;
        }
        if Self::is_keyframe(frame.flags) {
            self.keyframe_loss_pending = true;
        }
        self.missing_fragments = self.missing_fragments.saturating_add(u64::from(
            frame
                .fragment_count
                .saturating_sub(frame.fragments.len() as u16),
        ));
        self.resolved_fragments = self
            .resolved_fragments
            .saturating_add(u64::from(frame.fragment_count));
        self.drops = self.drops.saturating_add(1);
        self.partial_access_unit_drops = self.partial_access_unit_drops.saturating_add(1);
    }

    fn reset_pending_for_epoch(&mut self) {
        self.frames.clear();
        self.highest_observed_frame_id = None;
        self.frame_gaps.clear();
        self.expired_through_frame_id = None;
        self.rejected_frames.clear();
        self.terminal_frames.clear();
        self.reference_loss_pending = false;
        self.keyframe_loss_pending = false;
    }

    fn remember_terminal(&mut self, key: FrameKey) {
        let capacity = self.max_frames.saturating_mul(2).max(1);
        if self.terminal_frames.len() >= capacity {
            if let Some(oldest) = self
                .terminal_frames
                .iter()
                .min_by_key(|terminal| terminal.frame_id)
                .copied()
            {
                self.terminal_frames.remove(&oldest);
            }
        }
        self.terminal_frames.insert(key);
    }

    fn drop_oldest_rejected(&mut self) {
        if let Some(key) = self
            .rejected_frames
            .iter()
            .min_by_key(|key| key.frame_id)
            .copied()
        {
            self.rejected_frames.remove(&key);
        }
    }

    fn observe_frame_id(&mut self, frame_id: u64) -> Option<Instant> {
        let inferred_first_fragment_at =
            self.frame_gaps.remove(&frame_id).map(|gap| gap.noticed_at);
        if inferred_first_fragment_at.is_some() {
            tracing::debug!(frame_id, "previously missing access unit began arriving");
        }
        let Some(highest) = self.highest_observed_frame_id else {
            self.highest_observed_frame_id = Some(frame_id);
            return inferred_first_fragment_at;
        };
        if frame_id <= highest {
            return inferred_first_fragment_at;
        }

        let first_missing = highest.saturating_add(1);
        let missing_count = frame_id.saturating_sub(first_missing);
        let gap_capacity = self.max_frames.saturating_mul(2).max(1) as u64;
        if missing_count > gap_capacity {
            self.frame_gaps.clear();
            self.expired_through_frame_id = Some(
                self.expired_through_frame_id
                    .map_or(frame_id - 1, |expired| expired.max(frame_id - 1)),
            );
            self.reference_loss_pending = true;
            self.drops = self.drops.saturating_add(missing_count);
            self.whole_access_unit_gap_drops = self
                .whole_access_unit_gap_drops
                .saturating_add(missing_count);
            tracing::warn!(
                first_missing_frame_id = first_missing,
                next_frame_id = frame_id,
                missing_access_units = missing_count,
                "video frame-id gap exceeded bounded tracking capacity"
            );
        } else {
            let noticed_at = Instant::now();
            for missing_id in first_missing..frame_id {
                self.frame_gaps
                    .entry(missing_id)
                    .or_insert(PendingFrameGap { noticed_at });
            }
        }
        self.highest_observed_frame_id = Some(frame_id);
        inferred_first_fragment_at
    }
}

fn store_parity_shard(
    frame: &mut PartialFrame,
    group_start: u16,
    payload: Bytes,
) -> Result<(), ReassemblyError> {
    validate_parity_shard(frame.fragment_count, group_start, &payload)?;
    let parity_index = payload[0];
    let last_data_len = u16::from_be_bytes([payload[1], payload[2]]);
    let parity_bytes = payload.slice(FEC_PARITY_PREFIX_SIZE..);
    let key = (group_start, parity_index);
    if frame.parity_shards.contains_key(&key) {
        return Err(ReassemblyError::DuplicateFragment);
    }
    frame.parity_shards.insert(
        key,
        StoredParityShard {
            last_data_len,
            bytes: parity_bytes,
        },
    );
    Ok(())
}

fn validate_parity_shard(
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
    let valid_group = fec_group_ranges(fragment_count)
        .into_iter()
        .any(|group| group.start == group_start && group.len() >= 2);
    if !valid_group {
        return Err(ReassemblyError::InvalidFecParity);
    }
    Ok(())
}

fn recover_available_groups(frame: &mut PartialFrame) -> usize {
    let mut recovered_total = 0usize;
    for group in fec_group_ranges(frame.fragment_count) {
        if group.len() < 2 {
            continue;
        }
        let missing = group
            .clone()
            .filter(|index| !frame.fragments.contains_key(index))
            .collect::<Vec<_>>();
        if missing.is_empty() || missing.len() > FEC_PARITY_SHARDS {
            continue;
        }

        let parity = (0..FEC_PARITY_SHARDS)
            .map(|index| frame.parity_shards.get(&(group.start, index as u8)))
            .collect::<Vec<_>>();
        if missing.len() > parity.iter().filter(|shard| shard.is_some()).count() {
            continue;
        }
        let Some(last_data_len) = parity
            .iter()
            .flatten()
            .map(|shard| shard.last_data_len)
            .next()
        else {
            continue;
        };
        if parity
            .iter()
            .flatten()
            .any(|shard| shard.last_data_len != last_data_len)
        {
            continue;
        }

        let rebuilt = {
            let data = group
                .clone()
                .map(|index| frame.fragments.get(&index).map(Bytes::as_ref))
                .collect::<Vec<_>>();
            let parity = parity
                .iter()
                .map(|shard| shard.map(|shard| shard.bytes.as_ref()))
                .collect::<Vec<_>>();
            reconstruct_fec_group(&data, &parity, usize::from(last_data_len))
        };
        let Some(rebuilt) = rebuilt else {
            continue;
        };
        for (offset, shard) in rebuilt.into_iter().enumerate() {
            let index = group.start + offset as u16;
            if !missing.contains(&index) {
                continue;
            }
            let Some(shard) = shard else {
                continue;
            };
            frame.fragments.insert(index, Bytes::from(shard));
            frame.recovered_fragments.insert(index);
            recovered_total += 1;
        }
    }
    if recovered_total > 0 {
        tracing::debug!(
            recovered_fragments = recovered_total,
            frame_fragments = frame.fragment_count,
            "reconstructed video fragments from FEC parity"
        );
    }
    recovered_total
}

#[cfg(test)]
mod tests;
