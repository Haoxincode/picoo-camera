#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::path::{Path, PathBuf};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use super::SharedRingError;

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) struct KernelLockGuard {
    file: std::fs::File,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl Drop for KernelLockGuard {
    fn drop(&mut self) {
        // Rust 1.89+'s File lock API maps to flock on Unix and LockFileEx on
        // Windows. Keep the explicit unlock best-effort; closing the sidecar
        // descriptor/handle remains the crash-safe release mechanism.
        let _ = self.file.unlock();
    }
}

#[cfg(target_os = "macos")]
pub(super) fn try_macos_file_lock(
    lock_path: &Path,
    exclusive: bool,
) -> Result<Option<KernelLockGuard>, SharedRingError> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)
        .map_err(|error| map_file_err(lock_path, error))?;
    try_lock_open_file(file, lock_path, exclusive)
}

#[cfg(target_os = "windows")]
pub(super) fn try_windows_file_lock(
    lock_path: &Path,
    exclusive: bool,
) -> Result<Option<KernelLockGuard>, SharedRingError> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)
        .map_err(|error| map_file_err(lock_path, error))?;
    try_lock_open_file(file, lock_path, exclusive)
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn try_lock_open_file(
    file: std::fs::File,
    lock_path: &Path,
    exclusive: bool,
) -> Result<Option<KernelLockGuard>, SharedRingError> {
    use std::fs::TryLockError;

    let result = if exclusive {
        file.try_lock()
    } else {
        file.try_lock_shared()
    };
    match result {
        Ok(()) => Ok(Some(KernelLockGuard { file })),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(error)) => Err(map_file_err(lock_path, error)),
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) fn slot_lock_path(ring_path: &Path, index: usize) -> PathBuf {
    let mut path = ring_path.as_os_str().to_os_string();
    path.push(format!(".slot-{index}.lock"));
    PathBuf::from(path)
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) fn producer_lock_path(ring_path: &Path) -> PathBuf {
    let mut path = ring_path.as_os_str().to_os_string();
    path.push(".producer.lock");
    PathBuf::from(path)
}

#[cfg(target_os = "macos")]
pub(super) fn acquire_macos_producer_lock(
    ring_path: &Path,
) -> Result<KernelLockGuard, SharedRingError> {
    try_macos_file_lock(&producer_lock_path(ring_path), true)?
        .ok_or_else(|| SharedRingError::ProducerAlreadyRunning(ring_path.to_path_buf()))
}

#[cfg(target_os = "windows")]
pub(super) fn acquire_producer_lock(ring_path: &Path) -> Result<KernelLockGuard, SharedRingError> {
    let lock_path = producer_lock_path(ring_path);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    loop {
        if let Some(lock) = try_windows_file_lock(&lock_path, true)? {
            return Ok(lock);
        }
        if std::time::Instant::now() >= deadline {
            return Err(SharedRingError::ProducerAlreadyRunning(
                ring_path.to_path_buf(),
            ));
        }
        // Windows may release file locks slightly after the owning process
        // exits. Retry only this lifecycle boundary; per-slot frame locks
        // stay non-blocking.
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) fn map_file_err(path: &Path, error: std::io::Error) -> SharedRingError {
    SharedRingError::FileMapping {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}
