//! Cross-process Shared Frame Ring — REQ-PICOO-FRAME-003.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use lock::KernelLockGuard;
use thiserror::Error;

mod consumer;
mod content_fence;
mod demand;
pub use content_fence::RingContentFence;
mod layout;
mod lock;
mod mapping;
mod producer;
mod writer;

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod file_mapping;
#[cfg(target_os = "windows")]
mod windows_machine;

#[cfg(test)]
mod tests;

pub use consumer::SharedFrameRingConsumer;
pub use layout::{
    DEFAULT_MAX_FRAME_BYTES, PIXEL_FORMAT_NV12, RING_MAGIC, RING_META_SIZE, RING_READY_DONE,
    RING_SLOT_COUNT, RING_SLOT_META_SIZE,
};
pub use producer::{RingPublishOutcome, SharedFrameRingProducer};
#[cfg(target_os = "windows")]
pub use windows_machine::{windows_shared_ring_path, WINDOWS_SHARED_RING_DIRECTORY};
pub use writer::{
    SharedFrameRingWriter, SharedRingSubmitOutcome, SharedRingWriterEvent, SharedRingWriterStats,
};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum SharedFrameKind {
    #[default]
    Live = 0,
    Placeholder = 1,
}

impl SharedFrameKind {
    pub(super) const fn from_wire(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Live),
            1 => Some(Self::Placeholder),
            _ => None,
        }
    }

    const fn signal_bit(self) -> u64 {
        self as u64
    }
}

#[derive(Debug, Error)]
pub enum SharedRingError {
    #[error("shared memory: {0}")]
    Shmem(String),
    #[error("file mapping {path}: {message}")]
    FileMapping { path: PathBuf, message: String },
    #[error("a Shared Frame Ring producer is already active for {0}")]
    ProducerAlreadyRunning(PathBuf),
    #[error("invalid layout")]
    InvalidLayout,
    #[error("frame too large: {0} > max {1}")]
    FrameTooLarge(usize, usize),
    #[error("shared ring content was invalidated")]
    ContentInvalidated,
    #[error("invalid ring header")]
    InvalidHeader,
}

pub struct SharedFrameView<'a> {
    pub sequence: u64,
    pub timestamp_us: u64,
    pub kind: SharedFrameKind,
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
