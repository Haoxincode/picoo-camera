use std::sync::atomic::Ordering;

use shared_memory::ShmemConf;

use crate::hub::SLOT_COUNT;

use super::layout::{
    const_meta_at, const_slot_meta_at, const_slot_pixels_at, layout_size, validate_ring_header,
    PIXEL_FORMAT_NV12, RING_MAGIC, RING_READY_DONE, WRITER_LEASE,
};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use super::mapping::SlotLockAttempt;
use super::mapping::{map_shmem_err, ring_flink_path, ConsumerMapping, SharedMapping};
use super::{SharedFrameView, SharedRingError};

pub struct SharedFrameRingConsumer {
    mapping: ConsumerMapping,
    max_frame_bytes: usize,
}

// `Shmem` is conservative because it owns a raw mapping pointer. Moving the mapping between
// threads is safe: the OS mapping/handle is process-wide, `SharedFrameRingConsumer` never exposes
// ownership of the pointer, and every returned frame view is tied to `&self`. We intentionally do
// not implement `Sync`; callers that share a consumer must serialize access (the MF source does so
// with a mutex).
unsafe impl Send for SharedFrameRingConsumer {}

impl SharedFrameRingConsumer {
    pub fn open(name: &str, max_frame_bytes: usize) -> Result<Self, SharedRingError> {
        let flink = ring_flink_path(name);
        let shmem = ShmemConf::new()
            .size(layout_size(max_frame_bytes))
            .flink(&flink)
            .open()
            .map_err(map_shmem_err)?;
        let consumer = Self {
            mapping: ConsumerMapping::Shared(SharedMapping::new(shmem, flink)),
            max_frame_bytes,
        };
        consumer.validate_header()?;
        Ok(consumer)
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub fn open_file(
        path: impl AsRef<std::path::Path>,
        max_frame_bytes: usize,
    ) -> Result<Self, SharedRingError> {
        let path = path.as_ref();
        let mapping = super::file_mapping::open_file_mapping(path, max_frame_bytes)?;
        let consumer = Self {
            mapping: ConsumerMapping::File(mapping),
            max_frame_bytes,
        };
        Ok(consumer)
    }

    fn validate_header(&self) -> Result<(), SharedRingError> {
        validate_ring_header(self.mapping.as_ptr(), self.max_frame_bytes)
    }

    /// Returns false when a named ring's flink no longer points at this
    /// consumer's OS mapping. The caller must drop and reopen the consumer;
    /// file-backed macOS rings use their own inode-reconnect boundary.
    pub fn is_current_generation(&self) -> bool {
        match &self.mapping {
            ConsumerMapping::Shared(mapping) => mapping.is_current_generation(),
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            ConsumerMapping::File(mapping) => mapping.is_current_generation(),
        }
    }

    pub fn latest_frame(&self) -> Option<SharedFrameView<'_>> {
        let base = self.mapping.as_ptr();
        unsafe {
            let meta = &*const_meta_at(base);
            if meta.magic != RING_MAGIC {
                return None;
            }
            if meta.latest_sequence.load(Ordering::Acquire) == 0 {
                return None;
            }

            let mut candidates = Vec::with_capacity(SLOT_COUNT);
            for i in 0..SLOT_COUNT {
                let slot = &*const_slot_meta_at(base, self.max_frame_bytes, i);
                if slot.ready_state.load(Ordering::Acquire) != RING_READY_DONE {
                    continue;
                }
                let seq = slot.sequence.load(Ordering::Acquire);
                if seq == 0 {
                    continue;
                }
                candidates.push((i, seq));
            }

            candidates.sort_unstable_by(|(_, left), (_, right)| right.cmp(left));
            for (index, sequence) in candidates {
                // A newer slot can be exclusively locked by the producer for a
                // very short interval. Fall back to the next complete frame
                // instead of turning that contention into a black frame.
                if let Some(view) = self.read_view_at(base, index, sequence) {
                    return Some(view);
                }
            }
            None
        }
    }

    unsafe fn read_view_at<'a>(
        &'a self,
        base: *const u8,
        index: usize,
        sequence: u64,
    ) -> Option<SharedFrameView<'a>> {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let kernel_lock = match self.mapping.try_slot_lock(index).ok()? {
            #[cfg(target_os = "macos")]
            SlotLockAttempt::NotFile => None,
            SlotLockAttempt::Busy => return None,
            SlotLockAttempt::Acquired(lock) => {
                let slot = &*const_slot_meta_at(base, self.max_frame_bytes, index);
                // Shared ownership of this slot proves no file-backed writer
                // is alive. Recover a writer marker left by an abrupt exit.
                let _ = slot.reader_count.compare_exchange(
                    WRITER_LEASE,
                    0,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                );
                Some(lock)
            }
        };

        let view = read_view(base, self.max_frame_bytes, index, sequence)?;
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let view = attach_kernel_lock(view, kernel_lock);
        Some(view)
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn attach_kernel_lock<'a>(
    mut view: SharedFrameView<'a>,
    kernel_lock: Option<super::lock::KernelLockGuard>,
) -> SharedFrameView<'a> {
    view.kernel_lock = kernel_lock;
    view
}

unsafe fn read_view<'a>(
    base: *const u8,
    max_frame_bytes: usize,
    index: usize,
    sequence: u64,
) -> Option<SharedFrameView<'a>> {
    let slot = &*const_slot_meta_at(base, max_frame_bytes, index);
    if slot.ready_state.load(Ordering::Acquire) != RING_READY_DONE
        || slot.sequence.load(Ordering::Acquire) != sequence
    {
        return None;
    }
    let mut lease_state = slot.reader_count.load(Ordering::SeqCst);
    loop {
        if lease_state >= WRITER_LEASE - 1 {
            return None;
        }
        match slot.reader_count.compare_exchange_weak(
            lease_state,
            lease_state + 1,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => break,
            Err(current) => lease_state = current,
        }
    }
    if slot.ready_state.load(Ordering::Acquire) != RING_READY_DONE
        || slot.sequence.load(Ordering::Acquire) != sequence
    {
        slot.reader_count.fetch_sub(1, Ordering::SeqCst);
        return None;
    }
    let len = slot.data_length as usize;
    if len > max_frame_bytes || slot.pixel_format != PIXEL_FORMAT_NV12 {
        slot.reader_count.fetch_sub(1, Ordering::SeqCst);
        return None;
    }
    let pixels = const_slot_pixels_at(base, max_frame_bytes, index);
    Some(SharedFrameView {
        sequence,
        timestamp_us: slot.timestamp_us,
        width: slot.width,
        height: slot.height,
        stride: slot.stride,
        rotation: slot.rotation,
        nv12: &pixels[..len],
        reader_count: &slot.reader_count,
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        kernel_lock: None,
    })
}
