//! Cross-process Shared Frame Ring — REQ-PICOO-FRAME-003.

#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

#[cfg(target_os = "macos")]
use memmap2::{MmapMut, MmapOptions};
#[cfg(target_os = "macos")]
use objc2_foundation::{NSBundle, NSFileManager, NSString};
use shared_memory::{Shmem, ShmemConf, ShmemError};
use thiserror::Error;

use crate::hub::SLOT_COUNT;

pub const RING_MAGIC: u32 = 0x5049_434F; // "PICO"
pub const RING_VERSION: u32 = 2;
pub const PIXEL_FORMAT_NV12: u32 = 1;
#[cfg(target_os = "macos")]
pub const MACOS_APP_GROUP_INFO_KEY: &str = "PicooAppGroupIdentifier";

pub const DEFAULT_MAX_FRAME_BYTES: usize = 1920 * 1080 * 3 / 2;

pub const RING_META_SIZE: usize = 64;
pub const RING_SLOT_META_SIZE: usize = 64;
pub const RING_SLOT_COUNT: usize = SLOT_COUNT;

const READY_EMPTY: u32 = 0;
const READY_WRITING: u32 = 1;
pub const RING_READY_DONE: u32 = 2;
const WRITER_LEASE: u32 = u32::MAX;

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
    reader_count: AtomicU32,
    _pad: [u8; 16],
}

#[derive(Debug, Error)]
pub enum SharedRingError {
    #[error("shared memory: {0}")]
    Shmem(String),
    #[error("file mapping {path}: {message}")]
    FileMapping { path: PathBuf, message: String },
    #[error("macOS App Group container is unavailable: {0}")]
    AppGroupUnavailable(String),
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
    reader_count: &'a AtomicU32,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    kernel_lock: Option<KernelLockGuard>,
}

impl Drop for SharedFrameView<'_> {
    fn drop(&mut self) {
        self.reader_count.fetch_sub(1, Ordering::SeqCst);
    }
}

pub struct SharedFrameRingProducer {
    mapping: ProducerMapping,
    max_frame_bytes: usize,
}

pub struct SharedFrameRingConsumer {
    mapping: ConsumerMapping,
    max_frame_bytes: usize,
}

enum ProducerMapping {
    Shared(SharedMapping),
    #[cfg(target_os = "macos")]
    File(FileMapping),
}

enum ConsumerMapping {
    Shared(SharedMapping),
    #[cfg(target_os = "macos")]
    File(FileMapping),
}

struct SharedMapping {
    mapping: Shmem,
    #[cfg(target_os = "windows")]
    lock_root: PathBuf,
}

impl SharedMapping {
    fn new(mapping: Shmem, lock_root: PathBuf) -> Self {
        #[cfg(not(target_os = "windows"))]
        let _ = lock_root;
        Self {
            mapping,
            #[cfg(target_os = "windows")]
            lock_root,
        }
    }
}

#[cfg(target_os = "macos")]
struct FileMapping {
    mapping: MmapMut,
    path: PathBuf,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
struct KernelLockGuard {
    file: std::fs::File,
}

#[cfg(target_os = "macos")]
impl Drop for KernelLockGuard {
    fn drop(&mut self) {
        // SAFETY: The descriptor is owned by this individual lease. Unlock is
        // best-effort; closing the File immediately afterwards is the
        // crash-safe release mechanism.
        unsafe {
            use std::os::fd::AsRawFd;
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

#[cfg(target_os = "windows")]
impl Drop for KernelLockGuard {
    fn drop(&mut self) {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::UnlockFileEx;
        use windows_sys::Win32::System::IO::OVERLAPPED;

        let mut overlapped = OVERLAPPED::default();
        // SAFETY: This guard owns the live handle and the same byte range was
        // locked with an all-zero OVERLAPPED. Closing the File remains the
        // crash-safe fallback if explicit unlock fails.
        unsafe {
            UnlockFileEx(self.file.as_raw_handle(), 0, 1, 0, &mut overlapped);
        }
    }
}

#[cfg(target_os = "macos")]
impl FileMapping {
    fn try_slot_lock(
        &self,
        index: usize,
        exclusive: bool,
    ) -> Result<Option<KernelLockGuard>, SharedRingError> {
        use std::os::fd::AsRawFd;

        let lock_path = slot_lock_path(&self.path, index);
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|error| map_file_err(&lock_path, error))?;
        let operation = if exclusive {
            libc::LOCK_EX
        } else {
            libc::LOCK_SH
        } | libc::LOCK_NB;
        // SAFETY: The descriptor is live and remains owned by the returned
        // guard for exactly one slot lease.
        let result = unsafe { libc::flock(file.as_raw_fd(), operation) };
        if result == 0 {
            return Ok(Some(KernelLockGuard { file }));
        }
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::WouldBlock {
            return Ok(None);
        }
        Err(map_file_err(&lock_path, error))
    }
}

#[cfg(target_os = "windows")]
impl SharedMapping {
    fn try_slot_lock(
        &self,
        index: usize,
        exclusive: bool,
    ) -> Result<Option<KernelLockGuard>, SharedRingError> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::ERROR_LOCK_VIOLATION;
        use windows_sys::Win32::Storage::FileSystem::{
            LockFileEx, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY,
        };
        use windows_sys::Win32::System::IO::OVERLAPPED;

        let lock_path = slot_lock_path(&self.lock_root, index);
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|error| map_file_err(&lock_path, error))?;
        let flags = LOCKFILE_FAIL_IMMEDIATELY
            | if exclusive {
                LOCKFILE_EXCLUSIVE_LOCK
            } else {
                0
            };
        let mut overlapped = OVERLAPPED::default();
        // SAFETY: The descriptor is live and remains owned by the returned
        // guard for exactly one slot lease. Every lease locks byte zero only.
        let result = unsafe { LockFileEx(file.as_raw_handle(), flags, 0, 1, 0, &mut overlapped) };
        if result != 0 {
            return Ok(Some(KernelLockGuard { file }));
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_LOCK_VIOLATION as i32) {
            return Ok(None);
        }
        Err(map_file_err(&lock_path, error))
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
enum SlotLockAttempt {
    #[cfg(target_os = "macos")]
    NotFile,
    Busy,
    Acquired(KernelLockGuard),
}

impl ProducerMapping {
    fn as_ptr(&self) -> *mut u8 {
        match self {
            Self::Shared(mapping) => mapping.mapping.as_ptr(),
            #[cfg(target_os = "macos")]
            Self::File(mapping) => mapping.mapping.as_ptr().cast_mut(),
        }
    }
}

impl ConsumerMapping {
    fn as_ptr(&self) -> *const u8 {
        match self {
            Self::Shared(mapping) => mapping.mapping.as_ptr().cast_const(),
            #[cfg(target_os = "macos")]
            Self::File(mapping) => mapping.mapping.as_ptr(),
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl ProducerMapping {
    fn try_slot_lock(&self, index: usize) -> Result<SlotLockAttempt, SharedRingError> {
        match self {
            #[cfg(target_os = "macos")]
            Self::Shared(_) => Ok(SlotLockAttempt::NotFile),
            #[cfg(target_os = "windows")]
            Self::Shared(mapping) => Ok(match mapping.try_slot_lock(index, true)? {
                Some(lock) => SlotLockAttempt::Acquired(lock),
                None => SlotLockAttempt::Busy,
            }),
            #[cfg(target_os = "macos")]
            Self::File(mapping) => Ok(match mapping.try_slot_lock(index, true)? {
                Some(lock) => SlotLockAttempt::Acquired(lock),
                None => SlotLockAttempt::Busy,
            }),
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl ConsumerMapping {
    fn try_slot_lock(&self, index: usize) -> Result<SlotLockAttempt, SharedRingError> {
        match self {
            #[cfg(target_os = "macos")]
            Self::Shared(_) => Ok(SlotLockAttempt::NotFile),
            #[cfg(target_os = "windows")]
            Self::Shared(mapping) => Ok(match mapping.try_slot_lock(index, false)? {
                Some(lock) => SlotLockAttempt::Acquired(lock),
                None => SlotLockAttempt::Busy,
            }),
            #[cfg(target_os = "macos")]
            Self::File(mapping) => Ok(match mapping.try_slot_lock(index, false)? {
                Some(lock) => SlotLockAttempt::Acquired(lock),
                None => SlotLockAttempt::Busy,
            }),
        }
    }
}

// `Shmem` is conservative because it owns a raw mapping pointer. Moving the mapping between
// threads is safe: the OS mapping/handle is process-wide, `SharedFrameRingConsumer` never exposes
// ownership of the pointer, and every returned frame view is tied to `&self`. We intentionally do
// not implement `Sync`; callers that share a consumer must serialize access (the MF source does so
// with a mutex).
unsafe impl Send for SharedFrameRingConsumer {}

fn ring_flink_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("picoo-frame-ring-{name}.link"))
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn slot_lock_path(ring_path: &Path, index: usize) -> PathBuf {
    let mut path = ring_path.as_os_str().to_os_string();
    path.push(format!(".slot-{index}.lock"));
    PathBuf::from(path)
}

fn layout_size(max_frame_bytes: usize) -> usize {
    RING_META_SIZE + SLOT_COUNT * (RING_SLOT_META_SIZE + max_frame_bytes)
}

fn slot_offset(max_frame_bytes: usize, index: usize) -> usize {
    RING_META_SIZE + index * (RING_SLOT_META_SIZE + max_frame_bytes)
}

unsafe fn meta_at(base: *mut u8) -> *mut RingMeta {
    base.cast::<RingMeta>()
}

unsafe fn slot_meta_at(base: *mut u8, max_frame_bytes: usize, index: usize) -> *mut SlotMeta {
    (base.add(slot_offset(max_frame_bytes, index))).cast::<SlotMeta>()
}

unsafe fn const_meta_at(base: *const u8) -> *const RingMeta {
    base.cast::<RingMeta>()
}

unsafe fn const_slot_meta_at(
    base: *const u8,
    max_frame_bytes: usize,
    index: usize,
) -> *const SlotMeta {
    (base.add(slot_offset(max_frame_bytes, index))).cast::<SlotMeta>()
}

unsafe fn slot_pixels_at<'a>(base: *mut u8, max_frame_bytes: usize, index: usize) -> &'a mut [u8] {
    let start = slot_offset(max_frame_bytes, index) + RING_SLOT_META_SIZE;
    std::slice::from_raw_parts_mut(base.add(start), max_frame_bytes)
}

unsafe fn const_slot_pixels_at<'a>(
    base: *const u8,
    max_frame_bytes: usize,
    index: usize,
) -> &'a [u8] {
    let start = slot_offset(max_frame_bytes, index) + RING_SLOT_META_SIZE;
    std::slice::from_raw_parts(base.add(start), max_frame_bytes)
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
            mapping: ProducerMapping::Shared(SharedMapping::new(shmem, flink)),
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
                    mapping: ProducerMapping::Shared(SharedMapping::new(shmem, flink)),
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
            mapping: ProducerMapping::Shared(SharedMapping::new(shmem, flink)),
            max_frame_bytes,
        };
        producer.validate_header()?;
        Ok(producer)
    }

    fn init_header(&mut self) {
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

    fn validate_header(&self) -> Result<(), SharedRingError> {
        let base = self.mapping.as_ptr();
        unsafe {
            let meta = &*meta_at(base);
            if meta.magic != RING_MAGIC
                || meta.version != RING_VERSION
                || meta.slot_count != SLOT_COUNT as u32
            {
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

    #[cfg(target_os = "macos")]
    pub fn create_file(
        path: impl AsRef<Path>,
        max_frame_bytes: usize,
    ) -> Result<Self, SharedRingError> {
        let path = path.as_ref();
        let size = layout_size(max_frame_bytes);
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|error| map_file_err(path, error))?;
        file.set_len(size as u64)
            .map_err(|error| map_file_err(path, error))?;
        // SAFETY: The file is exclusively created, sized to the complete ring
        // layout, and kept alive by the mapping returned by the OS.
        let mapping = unsafe { MmapOptions::new().len(size).map_mut(&file) }
            .map_err(|error| map_file_err(path, error))?;
        let mut producer = Self {
            mapping: ProducerMapping::File(FileMapping {
                mapping,
                path: path.to_path_buf(),
            }),
            max_frame_bytes,
        };
        producer.init_header();
        Ok(producer)
    }

    #[cfg(target_os = "macos")]
    pub fn open_or_create_file(
        path: impl AsRef<Path>,
        max_frame_bytes: usize,
    ) -> Result<Self, SharedRingError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| map_file_err(parent, error))?;
        }
        match Self::create_file(path, max_frame_bytes) {
            Ok(producer) => Ok(producer),
            Err(SharedRingError::FileMapping { .. }) if path.is_file() => {
                match Self::open_file(path, max_frame_bytes) {
                    Ok(producer) => Ok(producer),
                    Err(SharedRingError::InvalidHeader | SharedRingError::InvalidLayout) => {
                        // The ring is a transient cache owned by Picoo. Replace
                        // stale ABI generations atomically by pathname; any old
                        // mapping remains valid until its process releases it.
                        std::fs::remove_file(path).map_err(|error| map_file_err(path, error))?;
                        Self::create_file(path, max_frame_bytes)
                    }
                    Err(error) => Err(error),
                }
            }
            Err(error) => Err(error),
        }
    }

    #[cfg(target_os = "macos")]
    pub fn open_file(
        path: impl AsRef<Path>,
        max_frame_bytes: usize,
    ) -> Result<Self, SharedRingError> {
        let path = path.as_ref();
        let size = layout_size(max_frame_bytes);
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|error| map_file_err(path, error))?;
        validate_file_size(path, &file, size)?;
        // SAFETY: Size validation guarantees the mapping covers the complete
        // fixed layout and the file remains referenced by the mapping.
        let mapping = unsafe { MmapOptions::new().len(size).map_mut(&file) }
            .map_err(|error| map_file_err(path, error))?;
        let producer = Self {
            mapping: ProducerMapping::File(FileMapping {
                mapping,
                path: path.to_path_buf(),
            }),
            max_frame_bytes,
        };
        producer.validate_header()?;
        Ok(producer)
    }
}

impl SharedFrameRingConsumer {
    pub fn open(name: &str, max_frame_bytes: usize) -> Result<Self, SharedRingError> {
        SharedFrameRingProducer::open(name, max_frame_bytes).map(|producer| Self {
            mapping: match producer.mapping {
                ProducerMapping::Shared(mapping) => ConsumerMapping::Shared(mapping),
                #[cfg(target_os = "macos")]
                ProducerMapping::File(_) => unreachable!("named open never creates a file mapping"),
            },
            max_frame_bytes: producer.max_frame_bytes,
        })
    }

    #[cfg(target_os = "macos")]
    pub fn open_file(
        path: impl AsRef<Path>,
        max_frame_bytes: usize,
    ) -> Result<Self, SharedRingError> {
        let path = path.as_ref();
        let size = layout_size(max_frame_bytes);
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|error| map_file_err(path, error))?;
        validate_file_size(path, &file, size)?;
        // SAFETY: The file size is validated. Consumers only mutate the
        // per-slot reader lease counter; frame metadata and pixels are read-only.
        let mapping = unsafe { MmapOptions::new().len(size).map_mut(&file) }
            .map_err(|error| map_file_err(path, error))?;
        let consumer = Self {
            mapping: ConsumerMapping::File(FileMapping {
                mapping,
                path: path.to_path_buf(),
            }),
            max_frame_bytes,
        };
        consumer.validate_header()?;
        Ok(consumer)
    }

    #[cfg(target_os = "macos")]
    fn validate_header(&self) -> Result<(), SharedRingError> {
        let base = self.mapping.as_ptr();
        // SAFETY: Both mapping backends cover at least RingMeta after their
        // constructors validate the complete layout size.
        let meta = unsafe { &*const_meta_at(base) };
        if meta.magic != RING_MAGIC
            || meta.version != RING_VERSION
            || meta.slot_count != SLOT_COUNT as u32
        {
            return Err(SharedRingError::InvalidHeader);
        }
        if meta.max_frame_bytes as usize != self.max_frame_bytes {
            return Err(SharedRingError::InvalidLayout);
        }
        Ok(())
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
    kernel_lock: Option<KernelLockGuard>,
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

fn map_shmem_err(err: ShmemError) -> SharedRingError {
    SharedRingError::Shmem(err.to_string())
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn map_file_err(path: &Path, error: std::io::Error) -> SharedRingError {
    SharedRingError::FileMapping {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}

#[cfg(target_os = "macos")]
fn validate_file_size(
    path: &Path,
    file: &std::fs::File,
    expected: usize,
) -> Result<(), SharedRingError> {
    let actual = file
        .metadata()
        .map_err(|error| map_file_err(path, error))?
        .len() as usize;
    if actual != expected {
        return Err(SharedRingError::InvalidLayout);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn macos_app_group_ring_path(name: &str) -> Result<PathBuf, SharedRingError> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(SharedRingError::AppGroupUnavailable(
            "invalid ring name".into(),
        ));
    }
    let identifier = macos_app_group_identifier()?;
    let manager = NSFileManager::defaultManager();
    let group = NSString::from_str(&identifier);
    let container = manager
        .containerURLForSecurityApplicationGroupIdentifier(&group)
        .ok_or_else(|| SharedRingError::AppGroupUnavailable(identifier.clone()))?;
    let path = container.path().ok_or_else(|| {
        SharedRingError::AppGroupUnavailable("container URL has no file path".into())
    })?;
    Ok(PathBuf::from(path.to_string()).join(format!("{name}.ring")))
}

#[cfg(target_os = "macos")]
pub fn macos_app_group_identifier() -> Result<String, SharedRingError> {
    if let Ok(identifier) = std::env::var("PICOO_APP_GROUP_IDENTIFIER") {
        if !identifier.trim().is_empty() {
            return Ok(identifier);
        }
    }

    let key = NSString::from_str(MACOS_APP_GROUP_INFO_KEY);
    let value = NSBundle::mainBundle()
        .objectForInfoDictionaryKey(&key)
        .and_then(|value| value.downcast::<NSString>().ok())
        .map(|value| value.to_string());
    value
        .filter(|identifier| !identifier.is_empty())
        .ok_or_else(|| {
            SharedRingError::AppGroupUnavailable(format!(
                "{MACOS_APP_GROUP_INFO_KEY} is absent from the host app Info.plist"
            ))
        })
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
        let _ = std::fs::remove_file(&path);
        #[cfg(target_os = "windows")]
        for index in 0..SLOT_COUNT {
            let _ = std::fs::remove_file(slot_lock_path(&path, index));
        }
    }

    #[cfg(target_os = "macos")]
    fn cleanup_file_ring(path: &Path) {
        let _ = std::fs::remove_file(path);
        for index in 0..SLOT_COUNT {
            let _ = std::fs::remove_file(slot_lock_path(path, index));
        }
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
    fn ring_layout_is_stable() {
        assert_eq!(std::mem::size_of::<RingMeta>(), RING_META_SIZE);
        assert_eq!(std::mem::size_of::<SlotMeta>(), RING_SLOT_META_SIZE);
        assert_eq!(std::mem::offset_of!(RingMeta, latest_sequence), 24);
        assert_eq!(std::mem::offset_of!(SlotMeta, ready_state), 40);
        assert_eq!(std::mem::offset_of!(SlotMeta, reader_count), 44);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_extension_sources_match_shared_ring_identity_and_abi() {
        let c_source =
            include_str!("../../../extensions/macos-camera-extension/SharedRingAtomic.c");
        let swift_source =
            include_str!("../../../extensions/macos-camera-extension/SharedRingReader.swift");
        let entitlements = include_str!(
            "../../../extensions/macos-camera-extension/PicooCameraExtension.entitlements"
        );

        for expected in [
            format!("PICOO_RING_VERSION = {RING_VERSION}"),
            format!("PICOO_RING_META_SIZE = {RING_META_SIZE}"),
            format!("PICOO_RING_SLOT_COUNT = {RING_SLOT_COUNT}"),
            format!("PICOO_RING_SLOT_META_SIZE = {RING_SLOT_META_SIZE}"),
            format!("PICOO_RING_READY_DONE = {RING_READY_DONE}"),
        ] {
            assert!(c_source.contains(&expected), "C ABI drift: {expected}");
        }
        assert!(swift_source.contains(MACOS_APP_GROUP_INFO_KEY));
        assert!(entitlements.contains("$(TeamIdentifierPrefix)com.haoxincode.picoo-camera"));
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
            // Interleave polls while overwriting (REQ-PICOO-FRAME-002).
            let _ = consumer.latest_frame();
        }
        let view = consumer.latest_frame().expect("latest");
        assert_eq!(view.sequence, last_seq);
        assert_eq!(view.timestamp_us, 31_000);
        cleanup(&name);
    }

    #[test]
    fn leased_slots_are_never_overwritten() {
        let name = test_ring_name();
        let max = nv12_byte_size(64, 64);
        let mut producer = SharedFrameRingProducer::create(&name, max).expect("create");
        let consumer = SharedFrameRingConsumer::open(&name, max).expect("open");
        let frame = nv12_black(64, 64);

        producer
            .publish_nv12(64, 64, 64, 0, 1, &frame)
            .expect("first");
        let first = consumer.latest_frame().expect("lease first");
        producer
            .publish_nv12(64, 64, 64, 0, 2, &frame)
            .expect("second");
        let second = consumer.latest_frame().expect("lease second");
        producer
            .publish_nv12(64, 64, 64, 0, 3, &frame)
            .expect("third");
        let third = consumer.latest_frame().expect("lease third");

        let unchanged = producer
            .publish_nv12(64, 64, 64, 0, 4, &frame)
            .expect("drop while all slots leased");
        assert_eq!(unchanged, third.sequence);
        assert_eq!(first.timestamp_us, 1);
        assert_eq!(second.timestamp_us, 2);
        assert_eq!(third.timestamp_us, 3);
        drop((first, second, third));
        cleanup(&name);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_kernel_lock_recovers_reader_lease_after_consumer_termination() {
        let name = test_ring_name();
        let max = nv12_byte_size(64, 64);
        let frame = nv12_black(64, 64);
        let mut producer = SharedFrameRingProducer::create(&name, max).expect("producer");
        producer
            .publish_nv12(64, 64, 64, 0, 1, &frame)
            .expect("first publish");

        let consumer = SharedFrameRingConsumer::open(&name, max).expect("consumer");
        let mut leaked = consumer.latest_frame().expect("leased frame");
        drop(leaked.kernel_lock.take());
        std::mem::forget(leaked);
        drop(consumer);
        // Model process termination: Windows closes the independent range-lock
        // handle, while the shared atomic count retains the abandoned lease.
        unsafe {
            (&*meta_at(producer.mapping.as_ptr()))
                .write_index
                .store(0, Ordering::SeqCst);
        }

        producer
            .publish_nv12(64, 64, 64, 0, 2, &frame)
            .expect("publish after terminated reader");
        let recovered = SharedFrameRingConsumer::open(&name, max).expect("new consumer");
        assert_eq!(recovered.latest_frame().expect("latest").timestamp_us, 2);

        drop(recovered);
        drop(producer);
        cleanup(&name);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_kernel_lock_recovers_writer_lease_after_producer_termination() {
        let name = test_ring_name();
        let max = nv12_byte_size(64, 64);
        let frame = nv12_black(64, 64);
        let mut producer = SharedFrameRingProducer::create(&name, max).expect("producer");
        producer
            .publish_nv12(64, 64, 64, 0, 1, &frame)
            .expect("publish");
        let writer_lock = match producer.mapping.try_slot_lock(0).expect("writer lock") {
            SlotLockAttempt::Acquired(lock) => lock,
            SlotLockAttempt::Busy => panic!("slot must lock"),
        };
        unsafe {
            (&*slot_meta_at(producer.mapping.as_ptr(), max, 0))
                .reader_count
                .store(WRITER_LEASE, Ordering::SeqCst);
        }
        drop(writer_lock);

        let consumer = SharedFrameRingConsumer::open(&name, max).expect("consumer");
        assert_eq!(
            consumer
                .latest_frame()
                .expect("recovered frame")
                .timestamp_us,
            1
        );

        drop(consumer);
        drop(producer);
        cleanup(&name);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_shared_slot_locks_remain_held_until_every_view_is_dropped() {
        let name = test_ring_name();
        let max = nv12_byte_size(64, 64);
        let frame = nv12_black(64, 64);
        let mut producer = SharedFrameRingProducer::create(&name, max).expect("producer");
        producer
            .publish_nv12(64, 64, 64, 0, 1, &frame)
            .expect("publish");
        let consumer = SharedFrameRingConsumer::open(&name, max).expect("consumer");
        let first = consumer.latest_frame().expect("first reader");
        let second = consumer.latest_frame().expect("second reader");

        assert!(matches!(
            producer.mapping.try_slot_lock(0).expect("writer attempt"),
            SlotLockAttempt::Busy
        ));
        drop(first);
        assert!(matches!(
            producer
                .mapping
                .try_slot_lock(0)
                .expect("writer attempt after one reader"),
            SlotLockAttempt::Busy
        ));
        drop(second);
        assert!(matches!(
            producer
                .mapping
                .try_slot_lock(0)
                .expect("writer attempt after all readers"),
            SlotLockAttempt::Acquired(_)
        ));

        drop(consumer);
        drop(producer);
        cleanup(&name);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_reader_falls_back_when_latest_slot_is_write_locked() {
        let name = test_ring_name();
        let max = nv12_byte_size(64, 64);
        let frame = nv12_black(64, 64);
        let mut producer = SharedFrameRingProducer::create(&name, max).expect("producer");
        producer
            .publish_nv12(64, 64, 64, 0, 1, &frame)
            .expect("first");
        producer
            .publish_nv12(64, 64, 64, 0, 2, &frame)
            .expect("second");
        let latest_lock = match producer.mapping.try_slot_lock(1).expect("latest lock") {
            SlotLockAttempt::Acquired(lock) => lock,
            SlotLockAttempt::Busy => panic!("latest slot must lock"),
        };

        let consumer = SharedFrameRingConsumer::open(&name, max).expect("consumer");
        let fallback = consumer.latest_frame().expect("fallback frame");
        assert_eq!(fallback.sequence, 1);
        assert_eq!(fallback.timestamp_us, 1);

        drop(fallback);
        drop(latest_lock);
        drop(consumer);
        drop(producer);
        cleanup(&name);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn file_backed_ring_roundtrip_matches_shared_layout() {
        let path = std::env::temp_dir().join(format!("{}.ring", test_ring_name()));
        let max = nv12_byte_size(64, 64);
        {
            let mut producer =
                SharedFrameRingProducer::open_or_create_file(&path, max).expect("file producer");
            let consumer = SharedFrameRingConsumer::open_file(&path, max).expect("file consumer");
            let frame = nv12_black(64, 64);
            producer
                .publish_nv12(64, 64, 64, 0, 42, &frame)
                .expect("publish");
            let view = consumer.latest_frame().expect("latest");
            assert_eq!(view.timestamp_us, 42);
            assert_eq!(view.nv12, frame);
        }
        cleanup_file_ring(&path);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn file_backed_ring_recovers_across_producer_restart_and_stale_abi() {
        let path = std::env::temp_dir().join(format!("{}.ring", test_ring_name()));
        let max = nv12_byte_size(64, 64);
        let frame = nv12_black(64, 64);

        {
            let mut producer =
                SharedFrameRingProducer::open_or_create_file(&path, max).expect("first producer");
            producer
                .publish_nv12(64, 64, 64, 0, 1, &frame)
                .expect("first publish");
        }
        {
            let mut restarted =
                SharedFrameRingProducer::open_or_create_file(&path, max).expect("restart");
            let consumer = SharedFrameRingConsumer::open_file(&path, max).expect("consumer");
            assert_eq!(
                consumer
                    .latest_frame()
                    .expect("preserved frame")
                    .timestamp_us,
                1
            );
            restarted
                .publish_nv12(64, 64, 64, 0, 2, &frame)
                .expect("publish after restart");
            assert_eq!(consumer.latest_frame().expect("new frame").timestamp_us, 2);
        }

        std::fs::write(&path, vec![0; layout_size(max)]).expect("stale ABI fixture");
        let mut recovered =
            SharedFrameRingProducer::open_or_create_file(&path, max).expect("replace stale ABI");
        recovered
            .publish_nv12(64, 64, 64, 0, 3, &frame)
            .expect("publish after ABI replacement");
        let consumer = SharedFrameRingConsumer::open_file(&path, max).expect("recovered consumer");
        assert_eq!(
            consumer
                .latest_frame()
                .expect("recovered frame")
                .timestamp_us,
            3
        );

        drop((consumer, recovered));
        cleanup_file_ring(&path);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn file_lock_recovers_reader_lease_after_consumer_termination() {
        let path = std::env::temp_dir().join(format!("{}.ring", test_ring_name()));
        let max = nv12_byte_size(64, 64);
        let frame = nv12_black(64, 64);
        let mut producer =
            SharedFrameRingProducer::open_or_create_file(&path, max).expect("producer");
        producer
            .publish_nv12(64, 64, 64, 0, 1, &frame)
            .expect("first publish");

        let consumer = SharedFrameRingConsumer::open_file(&path, max).expect("consumer");
        let mut leaked = consumer.latest_frame().expect("leased frame");
        drop(leaked.kernel_lock.take());
        std::mem::forget(leaked);
        drop(consumer);
        // The independent lease descriptor has been closed as the kernel
        // would do on process exit, while the shared atomic count remains.
        unsafe {
            (&*meta_at(producer.mapping.as_ptr()))
                .write_index
                .store(0, Ordering::SeqCst);
        }

        producer
            .publish_nv12(64, 64, 64, 0, 2, &frame)
            .expect("publish after terminated reader");
        let recovered = SharedFrameRingConsumer::open_file(&path, max).expect("new consumer");
        assert_eq!(recovered.latest_frame().expect("latest").timestamp_us, 2);

        drop((recovered, producer));
        cleanup_file_ring(&path);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn per_slot_file_locks_allow_parallel_write_and_multiple_views() {
        let path = std::env::temp_dir().join(format!("{}.ring", test_ring_name()));
        let max = nv12_byte_size(64, 64);
        let frame = nv12_black(64, 64);
        let mut producer =
            SharedFrameRingProducer::open_or_create_file(&path, max).expect("producer");
        let consumer = SharedFrameRingConsumer::open_file(&path, max).expect("consumer");

        producer
            .publish_nv12(64, 64, 64, 0, 1, &frame)
            .expect("first");
        let first_a = consumer.latest_frame().expect("first lease A");
        let first_b = consumer.latest_frame().expect("first lease B");
        producer
            .publish_nv12(64, 64, 64, 0, 2, &frame)
            .expect("second while slot zero is read");
        let second = consumer.latest_frame().expect("second lease");
        producer
            .publish_nv12(64, 64, 64, 0, 3, &frame)
            .expect("third while two slots are read");
        let third = consumer.latest_frame().expect("third lease");

        drop(first_a);
        let unchanged = producer
            .publish_nv12(64, 64, 64, 0, 4, &frame)
            .expect("all slots remain protected");
        assert_eq!(unchanged, third.sequence);

        drop(first_b);
        let fourth = producer
            .publish_nv12(64, 64, 64, 0, 4, &frame)
            .expect("slot zero becomes writable");
        assert_eq!(fourth, third.sequence + 1);
        assert_eq!(second.timestamp_us, 2);
        assert_eq!(third.timestamp_us, 3);

        drop(second);
        drop(third);
        drop(consumer);
        drop(producer);
        cleanup_file_ring(&path);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn reader_falls_back_when_latest_file_slot_is_write_locked() {
        let path = std::env::temp_dir().join(format!("{}.ring", test_ring_name()));
        let max = nv12_byte_size(64, 64);
        let frame = nv12_black(64, 64);
        let mut producer =
            SharedFrameRingProducer::open_or_create_file(&path, max).expect("producer");
        producer
            .publish_nv12(64, 64, 64, 0, 1, &frame)
            .expect("first");
        producer
            .publish_nv12(64, 64, 64, 0, 2, &frame)
            .expect("second");
        let latest_lock = match producer.mapping.try_slot_lock(1).expect("latest lock") {
            SlotLockAttempt::Acquired(lock) => lock,
            SlotLockAttempt::Busy | SlotLockAttempt::NotFile => panic!("file slot must lock"),
        };

        let consumer = SharedFrameRingConsumer::open_file(&path, max).expect("consumer");
        let fallback = consumer.latest_frame().expect("fallback frame");
        assert_eq!(fallback.sequence, 1);
        assert_eq!(fallback.timestamp_us, 1);

        drop(fallback);
        drop(latest_lock);
        drop(consumer);
        drop(producer);
        cleanup_file_ring(&path);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn file_lock_recovers_writer_lease_after_producer_termination() {
        let path = std::env::temp_dir().join(format!("{}.ring", test_ring_name()));
        let max = nv12_byte_size(64, 64);
        let frame = nv12_black(64, 64);
        {
            let mut producer =
                SharedFrameRingProducer::open_or_create_file(&path, max).expect("producer");
            producer
                .publish_nv12(64, 64, 64, 0, 1, &frame)
                .expect("publish");
            // Model a producer killed after acquiring its atomic lease. Its
            // file descriptor closes at scope exit, so the kernel lock cannot leak.
            let base = producer.mapping.as_ptr();
            unsafe {
                (&*slot_meta_at(base, max, 0))
                    .reader_count
                    .store(WRITER_LEASE, Ordering::SeqCst);
            }
        }

        let consumer = SharedFrameRingConsumer::open_file(&path, max).expect("consumer");
        assert_eq!(
            consumer
                .latest_frame()
                .expect("recovered frame")
                .timestamp_us,
            1
        );

        drop(consumer);
        cleanup_file_ring(&path);
    }
}
