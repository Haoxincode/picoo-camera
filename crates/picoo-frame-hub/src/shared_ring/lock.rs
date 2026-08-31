#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::path::{Path, PathBuf};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use super::SharedRingError;

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) struct KernelLockGuard {
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
pub(super) fn try_macos_file_lock(
    lock_path: &Path,
    exclusive: bool,
) -> Result<Option<KernelLockGuard>, SharedRingError> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;

    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .custom_flags(libc::O_NONBLOCK)
        .open(lock_path)
        .map_err(|error| map_file_err(lock_path, error))?;
    let operation = if exclusive {
        libc::LOCK_EX
    } else {
        libc::LOCK_SH
    } | libc::LOCK_NB;
    // SAFETY: The descriptor is live and remains owned by the returned guard.
    let result = unsafe { libc::flock(file.as_raw_fd(), operation) };
    if result == 0 {
        return Ok(Some(KernelLockGuard { file }));
    }
    let error = std::io::Error::last_os_error();
    if error.kind() == std::io::ErrorKind::WouldBlock {
        return Ok(None);
    }
    Err(map_file_err(lock_path, error))
}

#[cfg(target_os = "windows")]
pub(super) fn try_windows_file_lock(
    lock_path: &Path,
    exclusive: bool,
) -> Result<Option<KernelLockGuard>, SharedRingError> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::ERROR_LOCK_VIOLATION;
    use windows_sys::Win32::Storage::FileSystem::{
        LockFileEx, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY,
    };
    use windows_sys::Win32::System::IO::OVERLAPPED;

    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)
        .map_err(|error| map_file_err(lock_path, error))?;
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
    Err(map_file_err(lock_path, error))
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
        // Windows may release byte-range locks slightly after the owning
        // process exits. Retry only this lifecycle boundary; per-slot frame
        // locks stay non-blocking.
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
