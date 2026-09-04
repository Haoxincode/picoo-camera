use std::sync::atomic::{AtomicU32, AtomicU64};

use super::SharedRingError;

pub const RING_MAGIC: u32 = 0x5049_434F; // "PICO"
pub const RING_VERSION: u32 = 2;
pub const PIXEL_FORMAT_NV12: u32 = 1;

pub const DEFAULT_MAX_FRAME_BYTES: usize = 1920 * 1080 * 3 / 2;

pub const RING_META_SIZE: usize = 64;
pub const RING_SLOT_META_SIZE: usize = 64;
pub const RING_SLOT_COUNT: usize = 3;

pub(super) const READY_EMPTY: u32 = 0;
pub(super) const READY_WRITING: u32 = 1;
pub const RING_READY_DONE: u32 = 2;
pub(super) const WRITER_LEASE: u32 = u32::MAX;

#[repr(C)]
pub(super) struct RingMeta {
    pub(super) magic: u32,
    pub(super) version: u32,
    pub(super) slot_count: u32,
    pub(super) max_frame_bytes: u32,
    pub(super) write_index: AtomicU32,
    pub(super) latest_sequence: AtomicU64,
    pub(super) _pad: [u8; 32],
}

#[repr(C)]
pub(super) struct SlotMeta {
    pub(super) sequence: AtomicU64,
    pub(super) timestamp_us: u64,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) stride: u32,
    pub(super) rotation: u32,
    pub(super) pixel_format: u32,
    pub(super) data_length: u32,
    pub(super) ready_state: AtomicU32,
    pub(super) reader_count: AtomicU32,
    pub(super) _pad: [u8; 16],
}

pub(super) fn layout_size(max_frame_bytes: usize) -> usize {
    RING_META_SIZE + RING_SLOT_COUNT * slot_stride(max_frame_bytes)
}

fn slot_offset(max_frame_bytes: usize, index: usize) -> usize {
    RING_META_SIZE + index * slot_stride(max_frame_bytes)
}

fn slot_stride(max_frame_bytes: usize) -> usize {
    let unaligned = RING_SLOT_META_SIZE + max_frame_bytes;
    let alignment = std::mem::align_of::<SlotMeta>();
    unaligned.div_ceil(alignment) * alignment
}

pub(super) unsafe fn meta_at(base: *mut u8) -> *mut RingMeta {
    base.cast::<RingMeta>()
}

pub(super) unsafe fn slot_meta_at(
    base: *mut u8,
    max_frame_bytes: usize,
    index: usize,
) -> *mut SlotMeta {
    (base.add(slot_offset(max_frame_bytes, index))).cast::<SlotMeta>()
}

pub(super) unsafe fn const_meta_at(base: *const u8) -> *const RingMeta {
    base.cast::<RingMeta>()
}

pub(super) unsafe fn const_slot_meta_at(
    base: *const u8,
    max_frame_bytes: usize,
    index: usize,
) -> *const SlotMeta {
    (base.add(slot_offset(max_frame_bytes, index))).cast::<SlotMeta>()
}

pub(super) fn validate_ring_header(
    base: *const u8,
    max_frame_bytes: usize,
) -> Result<(), SharedRingError> {
    // SAFETY: Every caller has already opened a mapping large enough for the
    // requested ring layout.
    let meta = unsafe { &*const_meta_at(base) };
    if meta.magic != RING_MAGIC
        || meta.version != RING_VERSION
        || meta.slot_count != RING_SLOT_COUNT as u32
    {
        return Err(SharedRingError::InvalidHeader);
    }
    if meta.max_frame_bytes as usize != max_frame_bytes {
        return Err(SharedRingError::InvalidLayout);
    }
    Ok(())
}

pub(super) unsafe fn slot_pixels_at<'a>(
    base: *mut u8,
    max_frame_bytes: usize,
    index: usize,
) -> &'a mut [u8] {
    let start = slot_offset(max_frame_bytes, index) + RING_SLOT_META_SIZE;
    std::slice::from_raw_parts_mut(base.add(start), max_frame_bytes)
}

pub(super) unsafe fn const_slot_pixels_at<'a>(
    base: *const u8,
    max_frame_bytes: usize,
    index: usize,
) -> &'a [u8] {
    let start = slot_offset(max_frame_bytes, index) + RING_SLOT_META_SIZE;
    std::slice::from_raw_parts(base.add(start), max_frame_bytes)
}
