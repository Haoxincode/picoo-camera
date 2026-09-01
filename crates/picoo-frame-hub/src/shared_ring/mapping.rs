use std::path::PathBuf;

use shared_memory::{Shmem, ShmemError};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use super::file_mapping::FileMapping;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use super::lock::KernelLockGuard;
#[cfg(target_os = "windows")]
use super::lock::{slot_lock_path, try_windows_file_lock};
use super::SharedRingError;

pub(super) enum ProducerMapping {
    Shared(SharedMapping),
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    File(FileMapping),
}

pub(super) enum ConsumerMapping {
    Shared(SharedMapping),
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    File(FileMapping),
}

pub(super) struct SharedMapping {
    pub(super) mapping: Shmem,
    flink_path: PathBuf,
}

impl SharedMapping {
    pub(super) fn new(mapping: Shmem, flink_path: PathBuf) -> Self {
        Self {
            mapping,
            flink_path,
        }
    }

    pub(super) fn is_current_generation(&self) -> bool {
        std::fs::read_to_string(&self.flink_path)
            .map(|mapping_id| mapping_id == self.mapping.get_os_id())
            .unwrap_or(false)
    }
}

#[cfg(target_os = "windows")]
impl SharedMapping {
    fn try_slot_lock(
        &self,
        index: usize,
        exclusive: bool,
    ) -> Result<Option<KernelLockGuard>, SharedRingError> {
        try_windows_file_lock(&slot_lock_path(&self.flink_path, index), exclusive)
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) enum SlotLockAttempt {
    #[cfg(target_os = "macos")]
    NotFile,
    Busy,
    Acquired(KernelLockGuard),
}

impl ProducerMapping {
    pub(super) fn as_ptr(&self) -> *mut u8 {
        match self {
            Self::Shared(mapping) => mapping.mapping.as_ptr(),
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            Self::File(mapping) => mapping.mapping.as_ptr().cast_mut(),
        }
    }
}

impl ConsumerMapping {
    pub(super) fn as_ptr(&self) -> *const u8 {
        match self {
            Self::Shared(mapping) => mapping.mapping.as_ptr().cast_const(),
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            Self::File(mapping) => mapping.mapping.as_ptr(),
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl ProducerMapping {
    pub(super) fn try_slot_lock(&self, index: usize) -> Result<SlotLockAttempt, SharedRingError> {
        match self {
            #[cfg(target_os = "macos")]
            Self::Shared(_) => Ok(SlotLockAttempt::NotFile),
            #[cfg(target_os = "windows")]
            Self::Shared(mapping) => Ok(match mapping.try_slot_lock(index, true)? {
                Some(lock) => SlotLockAttempt::Acquired(lock),
                None => SlotLockAttempt::Busy,
            }),
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            Self::File(mapping) => Ok(match mapping.try_slot_lock(index, true)? {
                Some(lock) => SlotLockAttempt::Acquired(lock),
                None => SlotLockAttempt::Busy,
            }),
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl ConsumerMapping {
    pub(super) fn try_slot_lock(&self, index: usize) -> Result<SlotLockAttempt, SharedRingError> {
        match self {
            #[cfg(target_os = "macos")]
            Self::Shared(_) => Ok(SlotLockAttempt::NotFile),
            #[cfg(target_os = "windows")]
            Self::Shared(mapping) => Ok(match mapping.try_slot_lock(index, false)? {
                Some(lock) => SlotLockAttempt::Acquired(lock),
                None => SlotLockAttempt::Busy,
            }),
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            Self::File(mapping) => Ok(match mapping.try_slot_lock(index, false)? {
                Some(lock) => SlotLockAttempt::Acquired(lock),
                None => SlotLockAttempt::Busy,
            }),
        }
    }
}

pub(super) fn ring_flink_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("picoo-frame-ring-{name}.link"))
}

pub(super) fn map_shmem_err(err: ShmemError) -> SharedRingError {
    SharedRingError::Shmem(err.to_string())
}
