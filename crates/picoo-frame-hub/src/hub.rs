//! In-process three-slot FrameHub — REQ-PICOO-FRAME-001/002.

use bytes::Bytes;
use thiserror::Error;

pub const SLOT_COUNT: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadyState {
    Empty,
    Writing,
    Ready,
}

#[derive(Debug, Clone)]
pub struct FrameSlot {
    pub sequence: u64,
    pub timestamp_us: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub rotation: u32,
    pub pixel_data: Bytes,
    pub ready_state: ReadyState,
}

impl Default for FrameSlot {
    fn default() -> Self {
        Self {
            sequence: 0,
            timestamp_us: 0,
            width: 0,
            height: 0,
            stride: 0,
            rotation: 0,
            pixel_data: Bytes::new(),
            ready_state: ReadyState::Empty,
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FrameHubError {
    #[error("no empty slot available")]
    NoSlot,
}

pub struct FrameHub {
    slots: [FrameSlot; SLOT_COUNT],
    write_index: usize,
    latest_sequence: u64,
}

impl Default for FrameHub {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameHub {
    pub fn new() -> Self {
        Self {
            slots: std::array::from_fn(|_| FrameSlot::default()),
            write_index: 0,
            latest_sequence: 0,
        }
    }

    pub fn begin_write(&mut self) -> Result<usize, FrameHubError> {
        let index = self.write_index;
        if self.slots[index].ready_state == ReadyState::Writing {
            return Err(FrameHubError::NoSlot);
        }
        self.slots[index].ready_state = ReadyState::Writing;
        Ok(index)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn commit_write(
        &mut self,
        index: usize,
        width: u32,
        height: u32,
        stride: u32,
        rotation: u32,
        timestamp_us: u64,
        pixel_data: Bytes,
    ) {
        self.latest_sequence += 1;
        let slot = &mut self.slots[index];
        slot.sequence = self.latest_sequence;
        slot.timestamp_us = timestamp_us;
        slot.width = width;
        slot.height = height;
        slot.stride = stride;
        slot.rotation = rotation;
        slot.pixel_data = pixel_data;
        slot.ready_state = ReadyState::Ready;
        self.write_index = (index + 1) % SLOT_COUNT;
    }

    pub fn latest_ready(&self) -> Option<&FrameSlot> {
        self.slots
            .iter()
            .filter(|s| s.ready_state == ReadyState::Ready)
            .max_by_key(|s| s.sequence)
    }

    pub fn latest_sequence(&self) -> u64 {
        self.latest_sequence
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_latest_complete_frame() {
        let mut hub = FrameHub::new();
        let i0 = hub.begin_write().unwrap();
        hub.commit_write(i0, 1280, 720, 1280, 0, 1, Bytes::from_static(b"a"));
        let i1 = hub.begin_write().unwrap();
        hub.commit_write(i1, 1280, 720, 1280, 0, 2, Bytes::from_static(b"b"));
        assert_eq!(hub.latest_ready().unwrap().pixel_data.as_ref(), b"b");
    }

    #[test]
    fn overwrites_oldest_ready_slot_when_ring_is_full() {
        let mut hub = FrameHub::new();
        for seq in 1..=4 {
            let idx = hub.begin_write().unwrap();
            hub.commit_write(
                idx,
                1280,
                720,
                1280,
                0,
                seq,
                Bytes::from(vec![seq as u8; 4]),
            );
        }
        assert_eq!(hub.latest_sequence(), 4);
        let latest = hub.latest_ready().expect("latest");
        assert_eq!(latest.sequence, 4);
        assert_eq!(latest.pixel_data.as_ref(), &[4, 4, 4, 4]);
    }
}
