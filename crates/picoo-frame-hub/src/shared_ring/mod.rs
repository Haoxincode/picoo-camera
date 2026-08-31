//! Cross-process Shared Frame Ring — REQ-PICOO-FRAME-003.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use lock::KernelLockGuard;
use thiserror::Error;

mod consumer;
mod layout;
mod lock;
mod mapping;
mod producer;

#[cfg(target_os = "macos")]
mod macos_app_group;
#[cfg(target_os = "macos")]
mod macos_file;

#[cfg(test)]
mod tests;

pub use consumer::SharedFrameRingConsumer;
pub use layout::{
    DEFAULT_MAX_FRAME_BYTES, PIXEL_FORMAT_NV12, RING_MAGIC, RING_META_SIZE, RING_READY_DONE,
    RING_SLOT_COUNT, RING_SLOT_META_SIZE, RING_VERSION,
};
#[cfg(target_os = "macos")]
pub use macos_app_group::{
    macos_app_group_identifier, macos_app_group_ring_path, MACOS_APP_GROUP_INFO_KEY,
    MACOS_UNSIGNED_BUILD_INFO_KEY,
};
pub use producer::SharedFrameRingProducer;

#[derive(Debug, Error)]
pub enum SharedRingError {
    #[error("shared memory: {0}")]
    Shmem(String),
    #[error("file mapping {path}: {message}")]
    FileMapping { path: PathBuf, message: String },
    #[error("macOS App Group container is unavailable: {0}")]
    AppGroupUnavailable(String),
    #[error("a Shared Frame Ring producer is already active for {0}")]
    ProducerAlreadyRunning(PathBuf),
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
