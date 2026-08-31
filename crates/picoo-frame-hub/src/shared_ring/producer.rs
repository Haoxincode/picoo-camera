use std::path::PathBuf;
use std::sync::atomic::Ordering;

use shared_memory::{Shmem, ShmemConf, ShmemError};

use crate::hub::SLOT_COUNT;

use super::layout::{
    layout_size, meta_at, slot_meta_at, slot_pixels_at, validate_ring_header, PIXEL_FORMAT_NV12,
    READY_EMPTY, READY_WRITING, RING_MAGIC, RING_READY_DONE, RING_VERSION, WRITER_LEASE,
};
#[cfg(target_os = "windows")]
use super::lock::acquire_producer_lock;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use super::lock::KernelLockGuard;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use super::mapping::SlotLockAttempt;
use super::mapping::{map_shmem_err, ring_flink_path, ProducerMapping, SharedMapping};
use super::SharedRingError;

pub struct SharedFrameRingProducer {
    pub(super) mapping: ProducerMapping,
    pub(super) max_frame_bytes: usize,
    #[cfg(target_os = "windows")]
    pub(super) _producer_lock: KernelLockGuard,
    #[cfg(target_os = "macos")]
    pub(super) _producer_lock: Option<KernelLockGuard>,
}

impl SharedFrameRingProducer {
    #[cfg(target_os = "windows")]
    fn from_named_mapping(
        shmem: Shmem,
        flink: PathBuf,
        max_frame_bytes: usize,
        producer_lock: KernelLockGuard,
    ) -> Self {
        Self {
            mapping: ProducerMapping::Shared(SharedMapping::new(shmem, flink)),
            max_frame_bytes,
            _producer_lock: producer_lock,
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn from_named_mapping(shmem: Shmem, flink: PathBuf, max_frame_bytes: usize) -> Self {
        Self {
            mapping: ProducerMapping::Shared(SharedMapping::new(shmem, flink)),
            max_frame_bytes,
            #[cfg(target_os = "macos")]
            _producer_lock: None,
        }
    }

    pub fn create(name: &str, max_frame_bytes: usize) -> Result<Self, SharedRingError> {
        let flink = ring_flink_path(name);
        #[cfg(target_os = "windows")]
        let producer_lock = acquire_producer_lock(&flink)?;
        let size = layout_size(max_frame_bytes);
        let shmem = ShmemConf::new()
            .size(size)
            .flink(&flink)
            .create()
            .map_err(map_shmem_err)?;
        #[cfg(target_os = "windows")]
        let mut producer = Self::from_named_mapping(shmem, flink, max_frame_bytes, producer_lock);
        #[cfg(not(target_os = "windows"))]
        let mut producer = Self::from_named_mapping(shmem, flink, max_frame_bytes);
        producer.init_header();
        Ok(producer)
    }

    pub fn open_or_create(name: &str, max_frame_bytes: usize) -> Result<Self, SharedRingError> {
        let flink = ring_flink_path(name);
        let size = layout_size(max_frame_bytes);

        #[cfg(target_os = "windows")]
        {
            let producer_lock = acquire_producer_lock(&flink)?;
            match ShmemConf::new().size(size).flink(&flink).create() {
                Ok(shmem) => {
                    let mut producer =
                        Self::from_named_mapping(shmem, flink, max_frame_bytes, producer_lock);
                    producer.init_header();
                    Ok(producer)
                }
                Err(ShmemError::LinkExists) => {
                    match ShmemConf::new().size(size).flink(&flink).open() {
                        Ok(mut shmem) => {
                            // The lifecycle lock proves the former Producer is
                            // gone. Adopt cleanup ownership for this generation.
                            shmem.set_owner(true);
                            if validate_ring_header(shmem.as_ptr(), max_frame_bytes).is_ok() {
                                Ok(Self::from_named_mapping(
                                    shmem,
                                    flink,
                                    max_frame_bytes,
                                    producer_lock,
                                ))
                            } else {
                                // Drop the adopted invalid generation before
                                // creating its replacement. Otherwise its owner
                                // cleanup could unlink the replacement's flink.
                                drop(shmem);
                                let replacement = ShmemConf::new()
                                    .size(size)
                                    .flink(&flink)
                                    .create()
                                    .map_err(map_shmem_err)?;
                                let mut producer = Self::from_named_mapping(
                                    replacement,
                                    flink,
                                    max_frame_bytes,
                                    producer_lock,
                                );
                                producer.init_header();
                                Ok(producer)
                            }
                        }
                        Err(_) => {
                            let replacement = ShmemConf::new()
                                .size(size)
                                .flink(&flink)
                                .force_create_flink()
                                .create()
                                .map_err(map_shmem_err)?;
                            let mut producer = Self::from_named_mapping(
                                replacement,
                                flink,
                                max_frame_bytes,
                                producer_lock,
                            );
                            producer.init_header();
                            Ok(producer)
                        }
                    }
                }
                Err(error) => Err(map_shmem_err(error)),
            }
        }

        #[cfg(not(target_os = "windows"))]
        match ShmemConf::new().size(size).flink(&flink).create() {
            Ok(shmem) => {
                let mut producer = Self::from_named_mapping(shmem, flink, max_frame_bytes);
                producer.init_header();
                Ok(producer)
            }
            Err(ShmemError::LinkExists) => Self::open(name, max_frame_bytes),
            Err(e) => Err(map_shmem_err(e)),
        }
    }

    pub fn open(name: &str, max_frame_bytes: usize) -> Result<Self, SharedRingError> {
        let flink = ring_flink_path(name);
        #[cfg(target_os = "windows")]
        let producer_lock = acquire_producer_lock(&flink)?;
        let size = layout_size(max_frame_bytes);
        let shmem = ShmemConf::new()
            .size(size)
            .flink(&flink)
            .open()
            .map_err(map_shmem_err)?;
        #[cfg(target_os = "windows")]
        let shmem = {
            let mut shmem = shmem;
            shmem.set_owner(true);
            shmem
        };
        #[cfg(target_os = "windows")]
        let producer = Self::from_named_mapping(shmem, flink, max_frame_bytes, producer_lock);
        #[cfg(not(target_os = "windows"))]
        let producer = Self::from_named_mapping(shmem, flink, max_frame_bytes);
        producer.validate_header()?;
        Ok(producer)
    }

    pub(super) fn init_header(&mut self) {
        let base = self.mapping.as_ptr();
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
                slot.reader_count.store(0, Ordering::Relaxed);
            }
        }
    }

    pub(super) fn validate_header(&self) -> Result<(), SharedRingError> {
        validate_ring_header(self.mapping.as_ptr(), self.max_frame_bytes)
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

        let base = self.mapping.as_ptr();
        unsafe {
            let meta = &*meta_at(base);
            let start = meta.write_index.load(Ordering::Relaxed) as usize % SLOT_COUNT;
            let mut writable = None;
            for offset in 0..SLOT_COUNT {
                let index = (start + offset) % SLOT_COUNT;
                let slot = &mut *slot_meta_at(base, self.max_frame_bytes, index);
                #[cfg(any(target_os = "macos", target_os = "windows"))]
                let slot_lock = match self.mapping.try_slot_lock(index)? {
                    #[cfg(target_os = "macos")]
                    SlotLockAttempt::NotFile => None,
                    SlotLockAttempt::Busy => continue,
                    SlotLockAttempt::Acquired(lock) => {
                        // Exclusive ownership of this slot proves that any
                        // file-backed reader that left an atomic lease behind
                        // has exited and its kernel lock was released.
                        slot.reader_count.store(0, Ordering::SeqCst);
                        Some(lock)
                    }
                };
                if slot
                    .reader_count
                    .compare_exchange(0, WRITER_LEASE, Ordering::SeqCst, Ordering::SeqCst)
                    .is_err()
                {
                    continue;
                }
                slot.ready_state.store(READY_WRITING, Ordering::Release);
                #[cfg(any(target_os = "macos", target_os = "windows"))]
                {
                    writable = Some((index, slot, slot_lock));
                }
                #[cfg(not(any(target_os = "macos", target_os = "windows")))]
                {
                    writable = Some((index, slot));
                }
                break;
            }
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            let Some((index, slot, _slot_lock)) = writable
            else {
                return Ok(meta.latest_sequence.load(Ordering::Acquire));
            };
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            let Some((index, slot)) = writable
            else {
                // A consumer holding all three slots is slower than the
                // producer. Keep the previous complete frame instead of
                // blocking or overwriting memory being read.
                return Ok(meta.latest_sequence.load(Ordering::Acquire));
            };

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
            slot.ready_state.store(RING_READY_DONE, Ordering::Release);
            slot.reader_count.store(0, Ordering::SeqCst);
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
