//! Video fragment reassembly — REQ-PICOO-PROTOCOL-004.

use std::collections::HashMap;

use bytes::{Bytes, BytesMut};
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
    #[allow(dead_code)]
    flags: picoo_protocol::VideoPacketFlags,
    #[allow(dead_code)]
    pts_us: u64,
}

impl Default for PartialFrame {
    fn default() -> Self {
        Self {
            fragments: HashMap::new(),
            fragment_count: 0,
            flags: picoo_protocol::VideoPacketFlags::empty(),
            pts_us: 0,
        }
    }
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
    frames: HashMap<FrameKey, PartialFrame>,
    drops: u64,
}

impl ReassemblyMap {
    pub fn new(max_frames: usize, max_fragments: u16) -> Self {
        Self {
            max_frames,
            max_fragments,
            current_epoch: 0,
            frames: HashMap::new(),
            drops: 0,
        }
    }

    pub fn drop_count(&self) -> u64 {
        self.drops
    }

    pub fn ingest(&mut self, packet: VideoPacket) -> Result<Option<Bytes>, ReassemblyError> {
        if packet.fragment_count > self.max_fragments {
            return Err(ReassemblyError::TooManyFragments);
        }

        if packet.stream_epoch < self.current_epoch {
            return Ok(None);
        }

        if packet.stream_epoch > self.current_epoch {
            self.drops += self.frames.len() as u64;
            self.current_epoch = packet.stream_epoch;
            self.frames.clear();
        }

        if self.frames.len() >= self.max_frames
            && !self.frames.contains_key(&FrameKey {
                stream_epoch: packet.stream_epoch,
                frame_id: packet.frame_id,
            })
        {
            self.drop_oldest();
        }

        let key = FrameKey {
            stream_epoch: packet.stream_epoch,
            frame_id: packet.frame_id,
        };

        let entry = self.frames.entry(key).or_insert_with(|| PartialFrame {
            fragment_count: packet.fragment_count,
            flags: packet.flags,
            pts_us: packet.pts_us,
            ..Default::default()
        });

        if entry.fragment_count != packet.fragment_count {
            self.frames.remove(&key);
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
                self.frames.remove(&key);
                return Ok(None);
            }
        }

        self.frames.remove(&key);
        Ok(Some(assembled.freeze()))
    }

    pub fn is_keyframe(flags: VideoPacketFlags) -> bool {
        flags.contains(VideoPacketFlags::KEYFRAME)
    }

    fn drop_oldest(&mut self) {
        if let Some(key) = self.frames.keys().next().copied() {
            self.frames.remove(&key);
            self.drops += 1;
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
        assert_eq!(assembled.as_deref(), Some(&b"abcd"[..]));
    }

    #[test]
    fn isolates_stream_epochs() {
        let mut map = ReassemblyMap::new(8, 16);
        assert!(map.ingest(fragment(1, 10, 0, 2, b"ab")).unwrap().is_none());
        assert!(map.ingest(fragment(2, 10, 0, 1, b"xy")).unwrap().is_some());
    }
}
