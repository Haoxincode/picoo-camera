use std::path::{Path, PathBuf};

use memmap2::{MmapMut, MmapOptions};

use super::layout::{layout_size, validate_ring_header};
#[cfg(target_os = "macos")]
use super::lock::{acquire_macos_producer_lock, try_macos_file_lock};
#[cfg(target_os = "windows")]
use super::lock::{acquire_producer_lock, producer_lock_path, try_windows_file_lock};
use super::lock::{map_file_err, slot_lock_path, KernelLockGuard};
use super::mapping::ProducerMapping;
use super::producer::SharedFrameRingProducer;
use super::SharedRingError;

pub(super) struct FileMapping {
    pub(super) mapping: MmapMut,
    pub(super) path: PathBuf,
    identity: FileIdentity,
}

impl FileMapping {
    pub(super) fn try_slot_lock(
        &self,
        index: usize,
        exclusive: bool,
    ) -> Result<Option<KernelLockGuard>, SharedRingError> {
        let lock_path = slot_lock_path(&self.path, index);
        #[cfg(target_os = "macos")]
        {
            try_macos_file_lock(&lock_path, exclusive)
        }
        #[cfg(target_os = "windows")]
        {
            try_windows_file_lock(&lock_path, exclusive)
        }
    }

    pub(super) fn is_current_generation(&self) -> bool {
        let Ok(file) = std::fs::File::open(&self.path) else {
            // Replacement briefly renames the pathname before publishing the
            // next generation. Keep the complete mapped generation during
            // that gap; a later probe will compare the new file identity.
            return true;
        };
        file_identity(&file)
            .map(|identity| identity == self.identity)
            .unwrap_or(true)
    }

    #[cfg(target_os = "windows")]
    pub(super) fn has_live_producer(&self) -> bool {
        match try_windows_file_lock(&producer_lock_path(&self.path), false) {
            // The Producer owns an exclusive byte-range lock for its complete
            // lifetime. A shared probe can only succeed after it exits.
            Ok(Some(_unused_probe)) => false,
            Ok(None) => true,
            // A transient sidecar access error must not turn a live meeting
            // frame into a disconnected placeholder.
            Err(_) => true,
        }
    }
}

impl SharedFrameRingProducer {
    pub fn create_file(
        path: impl AsRef<Path>,
        max_frame_bytes: usize,
    ) -> Result<Self, SharedRingError> {
        let path = path.as_ref();
        let producer_lock = acquire_file_producer_lock(path)?;
        let mapping = create_file_mapping(path, max_frame_bytes)?;
        let mut producer = Self {
            mapping: ProducerMapping::File(mapping),
            max_frame_bytes,
            _producer_lock: Some(producer_lock),
        };
        producer.init_header();
        Ok(producer)
    }

    pub fn open_or_create_file(
        path: impl AsRef<Path>,
        max_frame_bytes: usize,
    ) -> Result<Self, SharedRingError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| map_file_err(parent, error))?;
        }
        let producer_lock = acquire_file_producer_lock(path)?;
        match create_file_mapping(path, max_frame_bytes) {
            Ok(mapping) => {
                let mut producer = Self {
                    mapping: ProducerMapping::File(mapping),
                    max_frame_bytes,
                    _producer_lock: Some(producer_lock),
                };
                producer.init_header();
                Ok(producer)
            }
            Err(SharedRingError::FileMapping { .. }) if path.is_file() => {
                match open_file_mapping(path, max_frame_bytes) {
                    Ok(mapping) => {
                        let producer = Self {
                            mapping: ProducerMapping::File(mapping),
                            max_frame_bytes,
                            _producer_lock: Some(producer_lock),
                        };
                        producer.validate_header()?;
                        Ok(producer)
                    }
                    Err(SharedRingError::InvalidHeader | SharedRingError::InvalidLayout) => {
                        replace_invalid_file_mapping(path, max_frame_bytes, producer_lock)
                    }
                    Err(error) => Err(error),
                }
            }
            Err(error) => Err(error),
        }
    }

    pub fn open_file(
        path: impl AsRef<Path>,
        max_frame_bytes: usize,
    ) -> Result<Self, SharedRingError> {
        let path = path.as_ref();
        let producer_lock = acquire_file_producer_lock(path)?;
        let mapping = open_file_mapping(path, max_frame_bytes)?;
        let producer = Self {
            mapping: ProducerMapping::File(mapping),
            max_frame_bytes,
            _producer_lock: Some(producer_lock),
        };
        producer.validate_header()?;
        Ok(producer)
    }
}

fn replace_invalid_file_mapping(
    path: &Path,
    max_frame_bytes: usize,
    producer_lock: KernelLockGuard,
) -> Result<SharedFrameRingProducer, SharedRingError> {
    #[cfg(target_os = "macos")]
    let mapping = {
        // The ring is a transient cache owned by Picoo. Replacing the pathname
        // gives the Camera Extension an inode-generation boundary.
        std::fs::remove_file(path).map_err(|error| map_file_err(path, error))?;
        create_file_mapping(path, max_frame_bytes)?
    };
    #[cfg(target_os = "windows")]
    let mapping = replace_windows_file_mapping(path, max_frame_bytes)?;

    let mut producer = SharedFrameRingProducer {
        mapping: ProducerMapping::File(mapping),
        max_frame_bytes,
        _producer_lock: Some(producer_lock),
    };
    producer.init_header();
    Ok(producer)
}

fn create_file_mapping(
    path: &Path,
    max_frame_bytes: usize,
) -> Result<FileMapping, SharedRingError> {
    #[cfg(target_os = "macos")]
    use std::os::unix::fs::OpenOptionsExt;
    #[cfg(target_os = "windows")]
    use std::os::windows::fs::OpenOptionsExt;

    let size = layout_size(max_frame_bytes);
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(target_os = "macos")]
    options.custom_flags(libc::O_NONBLOCK);
    #[cfg(target_os = "windows")]
    options.attributes(windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_TEMPORARY);
    let file = options
        .open(path)
        .map_err(|error| map_file_err(path, error))?;
    file.set_len(size as u64)
        .map_err(|error| map_file_err(path, error))?;
    // SAFETY: The file is exclusively created, sized to the complete ring
    // layout, and kept alive by the mapping returned by the OS.
    let mapping = unsafe { MmapOptions::new().len(size).map_mut(&file) }
        .map_err(|error| map_file_err(path, error))?;
    let identity = file_identity(&file).ok_or(SharedRingError::InvalidLayout)?;
    Ok(FileMapping {
        mapping,
        path: path.to_path_buf(),
        identity,
    })
}

#[cfg(target_os = "windows")]
fn replace_windows_file_mapping(
    path: &Path,
    max_frame_bytes: usize,
) -> Result<FileMapping, SharedRingError> {
    // Never resize or reinitialize a file that a live Frame Server consumer
    // may still map. Windows opens std files with FILE_SHARE_DELETE, so moving
    // the stale generation gives consumers an observable file-identity change
    // while their old mapping remains valid until they detach.
    let mut stale = path.as_os_str().to_os_string();
    stale.push(format!(
        ".stale-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let stale = PathBuf::from(stale);
    std::fs::rename(path, &stale).map_err(|error| map_file_err(path, error))?;
    let mapping = match create_file_mapping(path, max_frame_bytes) {
        Ok(mapping) => mapping,
        Err(error) => {
            let _ = std::fs::rename(&stale, path);
            return Err(error);
        }
    };
    // A mapped stale file may remain delete-pending until the old consumer
    // releases it. Failure is harmless: later maintenance can remove it.
    let _ = std::fs::remove_file(stale);
    Ok(mapping)
}

pub(super) fn open_file_mapping(
    path: &Path,
    max_frame_bytes: usize,
) -> Result<FileMapping, SharedRingError> {
    #[cfg(target_os = "macos")]
    use std::os::unix::fs::OpenOptionsExt;

    let size = layout_size(max_frame_bytes);
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true);
    #[cfg(target_os = "macos")]
    options.custom_flags(libc::O_NONBLOCK);
    let file = options
        .open(path)
        .map_err(|error| map_file_err(path, error))?;
    validate_file_size(path, &file, size)?;
    // SAFETY: Size validation guarantees the mapping covers the complete
    // fixed layout and the file remains referenced by the mapping.
    let mapping = unsafe { MmapOptions::new().len(size).map_mut(&file) }
        .map_err(|error| map_file_err(path, error))?;
    validate_ring_header(mapping.as_ptr(), max_frame_bytes)?;
    let identity = file_identity(&file).ok_or(SharedRingError::InvalidLayout)?;
    Ok(FileMapping {
        mapping,
        path: path.to_path_buf(),
        identity,
    })
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileIdentity {
    device: u64,
    inode: u64,
}

#[cfg(target_os = "macos")]
fn file_identity(file: &std::fs::File) -> Option<FileIdentity> {
    use std::os::unix::fs::MetadataExt;

    let metadata = file.metadata().ok()?;
    Some(FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileIdentity {
    volume_serial: u32,
    file_index: u64,
}

#[cfg(target_os = "windows")]
fn file_identity(file: &std::fs::File) -> Option<FileIdentity> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };

    let mut information = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::zeroed();
    // SAFETY: information points to writable storage and file owns a live
    // Windows handle for the duration of the call.
    let result =
        unsafe { GetFileInformationByHandle(file.as_raw_handle(), information.as_mut_ptr()) };
    if result == 0 {
        return None;
    }
    // SAFETY: A non-zero result initializes BY_HANDLE_FILE_INFORMATION.
    let information = unsafe { information.assume_init() };
    Some(FileIdentity {
        volume_serial: information.dwVolumeSerialNumber,
        file_index: (u64::from(information.nFileIndexHigh) << 32)
            | u64::from(information.nFileIndexLow),
    })
}

pub(super) fn validate_file_size(
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

fn acquire_file_producer_lock(path: &Path) -> Result<KernelLockGuard, SharedRingError> {
    #[cfg(target_os = "macos")]
    {
        acquire_macos_producer_lock(path)
    }
    #[cfg(target_os = "windows")]
    {
        acquire_producer_lock(path)
    }
}
