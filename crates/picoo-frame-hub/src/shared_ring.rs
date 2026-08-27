//! Cross-process Shared Frame Ring — REQ-PICOO-FRAME-003.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use shared_memory::{Shmem, ShmemConf, ShmemError};
use thiserror::Error;

use crate::hub::SLOT_COUNT;

pub const RING_MAGIC: u32 = 0x5049_434F; // "PICO"
pub const RING_VERSION: u32 = 1;
pub const PIXEL_FORMAT_NV12: u32 = 1;

pub const DEFAULT_MAX_FRAME_BYTES: usize = 1920 * 1080 * 3 / 2;

const META_SIZE: usize = 64;
const SLOT_META_SIZE: usize = 64;

const READY_EMPTY: u32 = 0;
const READY_WRITING: u32 = 1;
const READY_DONE: u32 = 2;

#[repr(C)]
struct RingMeta {
    magic: u32,
    version: u32,
    slot_count: u32,
    max_frame_bytes: u32,
    write_index: AtomicU32,
    latest_sequence: AtomicU64,
    _pad: [u8; 32],
}

#[repr(C)]
struct SlotMeta {
    sequence: AtomicU64,
    timestamp_us: u64,
    width: u32,
    height: u32,
    stride: u32,
    rotation: u32,
    pixel_format: u32,
    data_length: u32,
    ready_state: AtomicU32,
    _pad: [u8; 4],
}

#[derive(Debug, Error)]
pub enum SharedRingError {
    #[error("shared memory: {0}")]
    Shmem(String),
    #[error("invalid layout")]
    InvalidLayout,
    #[error("frame too large: {0} > max {1}")]
    FrameTooLarge(usize, usize),
    #[error("invalid magic/version")]
    InvalidHeader,
}

pub struct SharedFrameView<'a> {
    pub sequence: u64,
    pub timestamp_us: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub rotation: u32,
    pub nv12: &'a [u8],
}

pub struct SharedFrameRingProducer {
    shmem: Shmem,
    max_frame_bytes: usize,
}

pub struct SharedFrameRingConsumer {
    shmem: Shmem,
    max_frame_bytes: usize,
}

fn ring_flink_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("picoo-frame-ring-{name}.link"))
}

fn layout_size(max_frame_bytes: usize) -> usize {
    META_SIZE + SLOT_COUNT * (SLOT_META_SIZE + max_frame_bytes)
}

fn slot_offset(max_frame_bytes: usize, index: usize) -> usize {
    META_SIZE + index * (SLOT_META_SIZE + max_frame_bytes)
}

unsafe fn meta_at(base: *mut u8) -> *mut RingMeta {
    base.cast::<RingMeta>()
}

unsafe fn slot_meta_at(base: *mut u8, max_frame_bytes: usize, index: usize) -> *mut SlotMeta {
    (base.add(slot_offset(max_frame_bytes, index))).cast::<SlotMeta>()
}

unsafe fn slot_pixels_at<'a>(base: *mut u8, max_frame_bytes: usize, index: usize) -> &'a mut [u8] {
    let start = slot_offset(max_frame_bytes, index) + SLOT_META_SIZE;
    std::slice::from_raw_parts_mut(base.add(start), max_frame_bytes)
}

impl SharedFrameRingProducer {
    pub fn create(name: &str, max_frame_bytes: usize) -> Result<Self, SharedRingError> {
        let flink = ring_flink_path(name);
        let size = layout_size(max_frame_bytes);
        let shmem = ShmemConf::new()
            .size(size)
            .flink(&flink)
            .create()
            .map_err(map_shmem_err)?;
        let mut producer = Self {
            shmem,
            max_frame_bytes,
        };
        producer.init_header();
        Ok(producer)
    }

    pub fn open_or_create(name: &str, max_frame_bytes: usize) -> Result<Self, SharedRingError> {
        let flink = ring_flink_path(name);
        let size = layout_size(max_frame_bytes);
        match ShmemConf::new().size(size).flink(&flink).create() {
            Ok(shmem) => {
                let mut producer = Self {
                    shmem,
                    max_frame_bytes,
                };
                producer.init_header();
                Ok(producer)
            }
            Err(ShmemError::LinkExists) => Self::open(name, max_frame_bytes),
            Err(e) => Err(map_shmem_err(e)),
        }
    }

    pub fn open(name: &str, max_frame_bytes: usize) -> Result<Self, SharedRingError> {
        let flink = ring_flink_path(name);
        let size = layout_size(max_frame_bytes);
        let shmem = ShmemConf::new()
            .size(size)
            .flink(&flink)
            .open()
            .map_err(map_shmem_err)?;
        let producer = Self {
            shmem,
            max_frame_bytes,
        };
        producer.validate_header()?;
        Ok(producer)
    }

    fn init_header(&mut self) {
        let base = self.shmem.as_ptr();
        unsafe {
            let meta = &mut *meta_at(base);
            meta.magic = RING_MAGIC;
            meta.version = RING_VERSION;
            meta.slot_count = SLOT_COUNT as u32;
            meta.max_frame_bytes = self.max_frame_bytes as u32;
            meta.write_index.store(0, Ordering::Relaxed);
            meta.latest_sequence.store(0, Ordering::Relaxed);
            for i in 0..SLOT_COUNT {
                let slot = &mut *slot_meta_at(base, self.max_frame_bytes, i);
                slot.sequence.store(0, Ordering::Relaxed);
                slot.ready_state.store(READY_EMPTY, Ordering::Relaxed);
            }
        }
    }

    fn validate_header(&self) -> Result<(), SharedRingError> {
        let base = self.shmem.as_ptr();
        unsafe {
            let meta = &*meta_at(base);
            if meta.magic != RING_MAGIC || meta.version != RING_VERSION {
                return Err(SharedRingError::InvalidHeader);
            }
            if meta.max_frame_bytes as usize != self.max_frame_bytes {
                return Err(SharedRingError::InvalidLayout);
            }
        }
        Ok(())
    }

    pub fn publish_nv12(
        &mut self,
        width: u32,
        height: u32,
        stride: u32,
        rotation: u32,
        timestamp_us: u64,
        nv12: &[u8],
    ) -> Result<u64, SharedRingError> {
        if nv12.len() > self.max_frame_bytes {
            return Err(SharedRingError::FrameTooLarge(
                nv12.len(),
                self.max_frame_bytes,
            ));
        }

        let base = self.shmem.as_ptr();
        unsafe {
            let meta = &*meta_at(base);
            let index = meta.write_index.load(Ordering::Relaxed) as usize % SLOT_COUNT;
            let slot = &mut *slot_meta_at(base, self.max_frame_bytes, index);

            slot.ready_state.store(READY_WRITING, Ordering::Release);
            slot.timestamp_us = timestamp_us;
            slot.width = width;
            slot.height = height;
            slot.stride = stride;
            slot.rotation = rotation;
            slot.pixel_format = PIXEL_FORMAT_NV12;
            slot.data_length = nv12.len() as u32;

            let pixels = slot_pixels_at(base, self.max_frame_bytes, index);
            pixels.fill(0);
            pixels[..nv12.len()].copy_from_slice(nv12);

            let sequence = meta.latest_sequence.load(Ordering::Relaxed) + 1;
            slot.sequence.store(sequence, Ordering::Release);
            slot.ready_state.store(READY_DONE, Ordering::Release);
            meta.latest_sequence.store(sequence, Ordering::Release);
            meta.write_index
                .store((index as u32 + 1) % SLOT_COUNT as u32, Ordering::Release);
            Ok(sequence)
        }
    }

    pub fn flink_path(name: &str) -> PathBuf {
        ring_flink_path(name)
    }
}

impl SharedFrameRingConsumer {
    pub fn open(name: &str, max_frame_bytes: usize) -> Result<Self, SharedRingError> {
        SharedFrameRingProducer::open(name, max_frame_bytes).map(|producer| Self {
            shmem: producer.shmem,
            max_frame_bytes: producer.max_frame_bytes,
        })
    }

    pub fn latest_frame(&self) -> Option<SharedFrameView<'_>> {
        let base = self.shmem.as_ptr();
        unsafe {
            let meta = &*meta_at(base);
            if meta.magic != RING_MAGIC {
                return None;
            }
            let target_sequence = meta.latest_sequence.load(Ordering::Acquire);
            if target_sequence == 0 {
                return None;
            }

            let mut best: Option<(usize, u64)> = None;
            for i in 0..SLOT_COUNT {
                let slot = &*slot_meta_at(base, self.max_frame_bytes, i);
                if slot.ready_state.load(Ordering::Acquire) != READY_DONE {
                    continue;
                }
                let seq = slot.sequence.load(Ordering::Acquire);
                if seq == 0 {
                    continue;
                }
                if best.map(|(_, s)| seq > s).unwrap_or(true) {
                    best = Some((i, seq));
                }
            }

            let (index, sequence) = best?;
            if sequence != target_sequence {
                // Reader may still observe slightly stale slot; prefer exact latest.
                for i in 0..SLOT_COUNT {
                    let slot = &*slot_meta_at(base, self.max_frame_bytes, i);
                    if slot.sequence.load(Ordering::Acquire) == target_sequence
                        && slot.ready_state.load(Ordering::Acquire) == READY_DONE
                    {
                        return Some(read_view(base, self.max_frame_bytes, i, target_sequence));
                    }
                }
            }
            Some(read_view(base, self.max_frame_bytes, index, sequence))
        }
    }
}

unsafe fn read_view<'a>(
    base: *mut u8,
    max_frame_bytes: usize,
    index: usize,
    sequence: u64,
) -> SharedFrameView<'a> {
    let slot = &*slot_meta_at(base, max_frame_bytes, index);
    let len = slot.data_length as usize;
    let pixels = slot_pixels_at(base, max_frame_bytes, index);
    SharedFrameView {
        sequence,
        timestamp_us: slot.timestamp_us,
        width: slot.width,
        height: slot.height,
        stride: slot.stride,
        rotation: slot.rotation,
        nv12: &pixels[..len],
    }
}

fn map_shmem_err(err: ShmemError) -> SharedRingError {
    SharedRingError::Shmem(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::placeholder::{nv12_black, nv12_byte_size, PLACEHOLDER_HEIGHT, PLACEHOLDER_WIDTH};

    fn test_ring_name() -> String {
        format!(
            "test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )
    }

    fn cleanup(name: &str) {
        let path = SharedFrameRingProducer::flink_path(name);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn producer_consumer_roundtrip_in_two_handles() {
        let name = test_ring_name();
        let max = nv12_byte_size(PLACEHOLDER_WIDTH, PLACEHOLDER_HEIGHT);
        let mut producer = SharedFrameRingProducer::create(&name, max).expect("create");
        let consumer = SharedFrameRingConsumer::open(&name, max).expect("open");

        let frame = nv12_black(PLACEHOLDER_WIDTH, PLACEHOLDER_HEIGHT);
        let seq = producer
            .publish_nv12(
                PLACEHOLDER_WIDTH,
                PLACEHOLDER_HEIGHT,
                PLACEHOLDER_WIDTH,
                0,
                1,
                &frame,
            )
            .expect("publish");

        let view = consumer.latest_frame().expect("latest");
        assert_eq!(view.sequence, seq);
        assert_eq!(view.nv12.len(), frame.len());
        cleanup(&name);
    }

    #[test]
    fn open_or_create_allows_consumer_attach() {
        let name = test_ring_name();
        let max = nv12_byte_size(64, 64);
        let mut producer = SharedFrameRingProducer::open_or_create(&name, max).expect("create");
        let consumer = SharedFrameRingConsumer::open(&name, max).expect("consumer");
        let frame = nv12_black(64, 64);
        producer
            .publish_nv12(64, 64, 64, 0, 9, &frame)
            .expect("publish");
        assert_eq!(consumer.latest_frame().expect("view").timestamp_us, 9);
        cleanup(&name);
    }

    #[test]
    fn ring_layout_sizes_match_c_header() {
        assert_eq!(std::mem::size_of::<RingMeta>(), META_SIZE);
        // SlotMeta is 48 bytes; SLOT_META_SIZE reserves 64 bytes per slot header region.
        assert_eq!(std::mem::size_of::<SlotMeta>(), 48);
        assert_eq!(SLOT_META_SIZE, 64);
    }

    #[test]
    fn rapid_overwrite_consumer_sees_latest_sequence() {
        let name = test_ring_name();
        let max = nv12_byte_size(64, 64);
        let mut producer = SharedFrameRingProducer::create(&name, max).expect("create");
        let consumer = SharedFrameRingConsumer::open(&name, max).expect("open");
        let frame = nv12_black(64, 64);

        let mut last_seq = 0u64;
        for i in 0..32u64 {
            last_seq = producer
                .publish_nv12(64, 64, 64, 0, i * 1_000, &frame)
                .expect("publish");
        }
        let view = consumer.latest_frame().expect("latest");
        assert_eq!(view.sequence, last_seq);
        assert_eq!(view.timestamp_us, 31_000);
        cleanup(&name);
    }

    #[test]
    fn concurrent_publisher_and_poller() {
        use std::sync::Arc;
        use std::thread;

        let name = test_ring_name();
        let max = nv12_byte_size(64, 64);
        let mut producer = SharedFrameRingProducer::create(&name, max).expect("create");
        let consumer = SharedFrameRingConsumer::open(&name, max).expect("open");
        let stop = Arc::new(AtomicU32::new(0));
        let stop_reader = Arc::clone(&stop);

        let reader = thread::spawn(move || {
            let mut saw = 0u64;
            while stop_reader.load(Ordering::Acquire) == 0 {
                if let Some(view) = consumer.latest_frame() {
                    saw = saw.max(view.sequence);
                }
                thread::yield_now();
            }
            saw
        });

        let frame = nv12_black(64, 64);
        let mut last = 0u64;
        for i in 0..64u64 {
            last = producer
                .publish_nv12(64, 64, 64, 0, i, &frame)
                .expect("publish");
        }
        stop.store(1, Ordering::Release);
        let saw = reader.join().expect("join");
        assert!(saw > 0);
        assert!(saw <= last);
        cleanup(&name);
    }
}
