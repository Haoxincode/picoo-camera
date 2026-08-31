use super::*;
pub(super) use crate::placeholder::{
    nv12_black, nv12_byte_size, PLACEHOLDER_HEIGHT, PLACEHOLDER_WIDTH,
};
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(target_os = "windows")]
use super::lock::{producer_lock_path, slot_lock_path};
#[cfg(target_os = "windows")]
use crate::hub::SLOT_COUNT;

static TEST_RING_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn test_ring_name() -> String {
    format!(
        "test-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        TEST_RING_SEQUENCE.fetch_add(1, Ordering::Relaxed),
    )
}

fn cleanup(name: &str) {
    let path = SharedFrameRingProducer::flink_path(name);
    let _ = std::fs::remove_file(&path);
    #[cfg(target_os = "windows")]
    {
        for index in 0..SLOT_COUNT {
            let _ = std::fs::remove_file(slot_lock_path(&path, index));
        }
        let _ = std::fs::remove_file(producer_lock_path(&path));
    }
}

#[cfg(target_os = "macos")]
mod macos;
mod protocol;
#[cfg(target_os = "windows")]
mod windows;
