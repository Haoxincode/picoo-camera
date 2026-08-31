//! Video fragment reassembly — REQ-PICOO-PROTOCOL-004.

mod h264;

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use bytes::{Bytes, BytesMut};
pub use h264::{
    access_unit_to_annex_b, annex_b_parameter_sets, annex_b_to_length_prefixed, extract_sps_pps,
    is_length_prefixed_access_unit, length_prefixed_to_annex_b, split_annex_b_nals,
};
use picoo_protocol::{VideoPacket, VideoPacketFlags};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct FrameKey {
    stream_epoch: u32,
    frame_id: u64,
}

#[derive(Debug)]
struct PartialFrame {
    fragments: HashMap<u16, Bytes>,
    fragment_count: u16,
    flags: picoo_protocol::VideoPacketFlags,
    pts_us: u64,
    first_fragment_at: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssembledAccessUnit {
    pub data: Bytes,
    pub pts_us: u64,
    pub keyframe: bool,
    pub discardable: bool,
    pub stream_epoch: u32,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ReassemblyError {
    #[error("fragment_count exceeds limit")]
    TooManyFragments,
    #[error("duplicate fragment index")]
    DuplicateFragment,
    #[error("epoch mismatch")]
    EpochMismatch,
}

pub struct ReassemblyMap {
    max_frames: usize,
    max_fragments: u16,
    current_epoch: u32,
    expired_through_frame_id: Option<u64>,
    frames: HashMap<FrameKey, PartialFrame>,
    rejected_frames: HashSet<FrameKey>,
    terminal_frames: HashSet<FrameKey>,
    drops: u64,
    missing_fragments: u64,
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
            rejected_frames: HashSet::new(),
            terminal_frames: HashSet::new(),
            drops: 0,
            missing_fragments: 0,
            reference_loss_pending: false,
            keyframe_loss_pending: false,
        }
    }

    pub fn drop_count(&self) -> u64 {
        self.drops
    }

    /// Fragments known to be absent when an incomplete AU is expired or evicted.
    ///
    /// This deliberately excludes an AU for which no fragment arrived at all;
    /// observing that case requires a transport-wide packet sequence.
    pub fn missing_fragment_count(&self) -> u64 {
        self.missing_fragments
    }

    /// Discard pending media while preserving cumulative loss counters.
    /// Used when a decoder reference chain is reset inside the same session.
    pub fn clear_pending(&mut self) {
        let pending = self
            .frames
            .drain()
            .map(|(_, frame)| frame)
            .collect::<Vec<_>>();
        for frame in pending {
            self.record_incomplete_drop(frame);
        }
        self.expired_through_frame_id = None;
        self.rejected_frames.clear();
        self.terminal_frames.clear();
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
        let expired_through = self
            .frames
            .iter()
            .filter(|(_, frame)| frame.first_fragment_at <= deadline)
            .map(|(key, _)| key.frame_id)
            .max();
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
                self.expired_through_frame_id = Some(
                    self.expired_through_frame_id
                        .map_or(key.frame_id, |expired| expired.max(key.frame_id)),
                );
            }
        }
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
            self.clear_pending();
            self.current_epoch = packet.stream_epoch;
        }

        let key = FrameKey {
            stream_epoch: packet.stream_epoch,
            frame_id: packet.frame_id,
        };

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
        let packet_epoch = packet.stream_epoch;

        let entry = self.frames.entry(key).or_insert_with(|| PartialFrame {
            fragment_count: packet.fragment_count,
            flags: packet_flags,
            pts_us: packet_pts,
            fragments: HashMap::new(),
            first_fragment_at: Instant::now(),
        });

        if entry.fragment_count != packet.fragment_count {
            let frame = self.frames.remove(&key).expect("reassembly entry exists");
            self.record_incomplete_drop(frame);
            self.remember_terminal(key);
            return Ok(None);
        }

        if entry.fragments.contains_key(&packet.fragment_index) {
            return Err(ReassemblyError::DuplicateFragment);
        }

        entry
            .fragments
            .insert(packet.fragment_index, packet.payload);

        if entry.fragments.len() as u16 != entry.fragment_count {
            return Ok(None);
        }

        let mut assembled =
            BytesMut::with_capacity(entry.fragments.values().map(|p| p.len()).sum());
        for index in 0..entry.fragment_count {
            if let Some(chunk) = entry.fragments.get(&index) {
                assembled.extend_from_slice(chunk);
            } else {
                let frame = self.frames.remove(&key).expect("reassembly entry exists");
                self.record_incomplete_drop(frame);
                self.remember_terminal(key);
                return Ok(None);
            }
        }

        let flags = entry.flags;
        let pts_us = entry.pts_us;
        self.frames.remove(&key);
        self.remember_terminal(key);
        Ok(Some(AssembledAccessUnit {
            data: assembled.freeze(),
            pts_us,
            keyframe: Self::is_keyframe(flags),
            discardable: flags.contains(VideoPacketFlags::DISCARDABLE),
            stream_epoch: packet_epoch,
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
        self.drops = self.drops.saturating_add(1);
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use picoo_protocol::VideoPacket;

    fn fragment(
        epoch: u32,
        frame_id: u64,
        index: u16,
        count: u16,
        payload: &'static [u8],
    ) -> VideoPacket {
        VideoPacket {
            version: VideoPacket::VERSION,
            flags: VideoPacketFlags::empty(),
            stream_epoch: epoch,
            frame_id,
            pts_us: 0,
            fragment_index: index,
            fragment_count: count,
            payload: Bytes::copy_from_slice(payload),
        }
    }

    #[test]
    fn reassembles_fragments_same_epoch() {
        let mut map = ReassemblyMap::new(8, 16);
        assert!(map.ingest(fragment(1, 10, 0, 2, b"ab")).unwrap().is_none());
        let assembled = map.ingest(fragment(1, 10, 1, 2, b"cd")).unwrap();
        assert_eq!(
            assembled.as_ref().map(|a| a.data.as_ref()),
            Some(&b"abcd"[..])
        );
        assert_eq!(map.missing_fragment_count(), 0);
    }

    #[test]
    fn isolates_stream_epochs() {
        let mut map = ReassemblyMap::new(8, 16);
        assert!(map.ingest(fragment(1, 10, 0, 2, b"ab")).unwrap().is_none());
        assert!(map.ingest(fragment(2, 10, 0, 1, b"xy")).unwrap().is_some());
    }

    #[test]
    fn dropping_incomplete_keyframe_sets_loss_flag() {
        let mut map = ReassemblyMap::new(1, 16);
        let key = VideoPacket {
            version: VideoPacket::VERSION,
            flags: VideoPacketFlags::KEYFRAME,
            stream_epoch: 1,
            frame_id: 1,
            pts_us: 0,
            fragment_index: 0,
            fragment_count: 2,
            payload: Bytes::copy_from_slice(b"k0"),
        };
        assert!(map.ingest(key).unwrap().is_none());
        // Force drop of the incomplete keyframe by admitting another frame.
        let other = fragment(1, 2, 0, 1, b"z");
        assert!(map.ingest(other).unwrap().is_some());
        assert!(map.take_keyframe_loss());
        assert!(!map.take_keyframe_loss());
    }

    #[test]
    fn reassembly_deadline_reports_and_discards_incomplete_keyframe() {
        let mut map = ReassemblyMap::new(8, 16);
        let key_head = VideoPacket {
            version: VideoPacket::VERSION,
            flags: VideoPacketFlags::KEYFRAME | VideoPacketFlags::START_OF_ACCESS_UNIT,
            stream_epoch: 1,
            frame_id: 1,
            pts_us: 0,
            fragment_index: 0,
            fragment_count: 2,
            payload: Bytes::copy_from_slice(b"k0"),
        };
        assert!(map.ingest(key_head).unwrap().is_none());

        map.expire_incomplete_older_than(Instant::now(), Duration::ZERO);
        assert!(map.take_keyframe_loss());
        assert_eq!(map.missing_fragment_count(), 1);

        let key_tail = VideoPacket {
            version: VideoPacket::VERSION,
            flags: VideoPacketFlags::KEYFRAME | VideoPacketFlags::END_OF_ACCESS_UNIT,
            stream_epoch: 1,
            frame_id: 1,
            pts_us: 0,
            fragment_index: 1,
            fragment_count: 2,
            payload: Bytes::copy_from_slice(b"k1"),
        };
        assert!(map.ingest(key_tail).unwrap().is_none());
        assert!(!map.take_keyframe_loss());
        assert_eq!(map.drop_count(), 1, "loss must be counted once");
        assert_eq!(map.missing_fragment_count(), 1, "loss must be counted once");
    }

    #[test]
    fn clearing_pending_preserves_same_unit_fragment_loss_counters() {
        let mut map = ReassemblyMap::new(8, 16);
        assert!(map.ingest(fragment(1, 1, 0, 3, b"a")).unwrap().is_none());
        assert!(map.ingest(fragment(1, 1, 2, 3, b"c")).unwrap().is_none());

        map.clear_pending();

        assert_eq!(map.drop_count(), 1);
        assert_eq!(map.missing_fragment_count(), 1);
        assert!(map.take_reference_chain_loss());
        assert!(map.ingest(fragment(1, 2, 0, 1, b"next")).unwrap().is_some());
        assert_eq!(map.missing_fragment_count(), 1);
    }

    #[test]
    fn incomplete_delta_requires_refresh_unless_marked_discardable() {
        let mut map = ReassemblyMap::new(8, 16);
        assert!(map.ingest(fragment(1, 1, 0, 2, b"p0")).unwrap().is_none());
        map.expire_incomplete_older_than(Instant::now(), Duration::ZERO);
        assert!(map.take_reference_chain_loss());

        let mut discardable = fragment(1, 2, 0, 2, b"b0");
        discardable.flags = VideoPacketFlags::DISCARDABLE;
        assert!(map.ingest(discardable).unwrap().is_none());
        map.expire_incomplete_older_than(Instant::now(), Duration::ZERO);
        assert!(!map.take_reference_chain_loss());
    }

    #[test]
    fn cross_access_unit_reordering_completes_both_frames_before_deadline() {
        let mut map = ReassemblyMap::new(8, 16);
        let old_head = VideoPacket {
            version: VideoPacket::VERSION,
            flags: VideoPacketFlags::KEYFRAME | VideoPacketFlags::START_OF_ACCESS_UNIT,
            stream_epoch: 1,
            frame_id: 1,
            pts_us: 1,
            fragment_index: 0,
            fragment_count: 2,
            payload: Bytes::copy_from_slice(b"k0"),
        };
        assert!(map.ingest(old_head).unwrap().is_none());

        let mut newer = fragment(1, 2, 0, 1, b"p");
        newer.flags = VideoPacketFlags::START_OF_ACCESS_UNIT | VideoPacketFlags::END_OF_ACCESS_UNIT;
        assert_eq!(
            map.ingest(newer)
                .unwrap()
                .as_ref()
                .map(|au| au.data.as_ref()),
            Some(&b"p"[..])
        );

        let mut old_tail = fragment(1, 1, 1, 2, b"k1");
        old_tail.flags = VideoPacketFlags::KEYFRAME | VideoPacketFlags::END_OF_ACCESS_UNIT;
        assert_eq!(
            map.ingest(old_tail)
                .unwrap()
                .as_ref()
                .map(|au| au.data.as_ref()),
            Some(&b"k0k1"[..])
        );
        assert!(!map.take_keyframe_loss());
    }

    #[test]
    fn capacity_evicts_oldest_non_keyframe_before_keyframe() {
        let mut map = ReassemblyMap::new(2, 16);
        let mut key_head = fragment(1, 1, 0, 2, b"k0");
        key_head.flags = VideoPacketFlags::KEYFRAME;
        assert!(map.ingest(key_head).unwrap().is_none());
        assert!(map.ingest(fragment(1, 2, 0, 2, b"p0")).unwrap().is_none());
        assert!(map.ingest(fragment(1, 3, 0, 1, b"new")).unwrap().is_some());
        assert!(!map.take_keyframe_loss());
        assert!(map.ingest(fragment(1, 2, 1, 2, b"p1")).unwrap().is_none());
        assert_eq!(map.drop_count(), 1, "evicted frame must stay terminal");

        let mut key_tail = fragment(1, 1, 1, 2, b"k1");
        key_tail.flags = VideoPacketFlags::KEYFRAME;
        assert_eq!(
            map.ingest(key_tail)
                .unwrap()
                .as_ref()
                .map(|au| au.data.as_ref()),
            Some(&b"k0k1"[..])
        );
    }

    #[test]
    fn oversized_keyframe_is_counted_and_requests_idr_once() {
        let mut map = ReassemblyMap::new(2, 2);
        let mut first = fragment(1, 1, 0, 3, b"k0");
        first.flags = VideoPacketFlags::KEYFRAME | VideoPacketFlags::START_OF_ACCESS_UNIT;
        assert_eq!(map.ingest(first), Err(ReassemblyError::TooManyFragments));
        assert_eq!(map.drop_count(), 1);
        assert!(map.take_keyframe_loss());

        let mut second = fragment(1, 1, 1, 3, b"k1");
        second.flags = VideoPacketFlags::KEYFRAME;
        assert_eq!(map.ingest(second), Err(ReassemblyError::TooManyFragments));
        assert_eq!(map.drop_count(), 1);
        assert!(!map.take_keyframe_loss());
    }

    #[test]
    fn late_duplicate_cannot_recreate_a_completed_frame() {
        let mut map = ReassemblyMap::new(2, 2);
        assert!(map.ingest(fragment(1, 1, 0, 1, b"done")).unwrap().is_some());
        assert!(map.ingest(fragment(1, 1, 0, 1, b"late")).unwrap().is_none());
        assert_eq!(map.drop_count(), 0);
    }
}
