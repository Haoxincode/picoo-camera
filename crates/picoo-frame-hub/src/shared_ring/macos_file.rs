use std::path::{Path, PathBuf};

use memmap2::{MmapMut, MmapOptions};

use super::layout::{layout_size, validate_ring_header};
use super::lock::KernelLockGuard;
use super::lock::{acquire_macos_producer_lock, map_file_err, slot_lock_path, try_macos_file_lock};
use super::mapping::ProducerMapping;
use super::producer::SharedFrameRingProducer;
use super::SharedRingError;

pub(super) struct FileMapping {
    pub(super) mapping: MmapMut,
    pub(super) path: PathBuf,
}

impl FileMapping {
    pub(super) fn try_slot_lock(
        &self,
        index: usize,
        exclusive: bool,
    ) -> Result<Option<KernelLockGuard>, SharedRingError> {
        let lock_path = slot_lock_path(&self.path, index);
        try_macos_file_lock(&lock_path, exclusive)
    }
}

impl SharedFrameRingProducer {
    pub fn create_file(
        path: impl AsRef<Path>,
        max_frame_bytes: usize,
    ) -> Result<Self, SharedRingError> {
        let path = path.as_ref();
        let producer_lock = acquire_macos_producer_lock(path)?;
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
        let producer_lock = acquire_macos_producer_lock(path)?;
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
                        // The ring is a transient cache owned by Picoo. Replace
                        // stale ABI generations atomically by pathname; any old
                        // mapping remains valid until its process releases it.
                        std::fs::remove_file(path).map_err(|error| map_file_err(path, error))?;
                        let mapping = create_file_mapping(path, max_frame_bytes)?;
                        let mut producer = Self {
                            mapping: ProducerMapping::File(mapping),
                            max_frame_bytes,
                            _producer_lock: Some(producer_lock),
                        };
                        producer.init_header();
                        Ok(producer)
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
        let producer_lock = acquire_macos_producer_lock(path)?;
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

fn create_file_mapping(
    path: &Path,
    max_frame_bytes: usize,
) -> Result<FileMapping, SharedRingError> {
    use std::os::unix::fs::OpenOptionsExt;

    let size = layout_size(max_frame_bytes);
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .map_err(|error| map_file_err(path, error))?;
    file.set_len(size as u64)
        .map_err(|error| map_file_err(path, error))?;
    // SAFETY: The file is exclusively created, sized to the complete ring
    // layout, and kept alive by the mapping returned by the OS.
    let mapping = unsafe { MmapOptions::new().len(size).map_mut(&file) }
        .map_err(|error| map_file_err(path, error))?;
    Ok(FileMapping {
        mapping,
        path: path.to_path_buf(),
    })
}

pub(super) fn open_file_mapping(
    path: &Path,
    max_frame_bytes: usize,
) -> Result<FileMapping, SharedRingError> {
    use std::os::unix::fs::OpenOptionsExt;

    let size = layout_size(max_frame_bytes);
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .map_err(|error| map_file_err(path, error))?;
    validate_file_size(path, &file, size)?;
    // SAFETY: Size validation guarantees the mapping covers the complete
    // fixed layout and the file remains referenced by the mapping.
    let mapping = unsafe { MmapOptions::new().len(size).map_mut(&file) }
        .map_err(|error| map_file_err(path, error))?;
    validate_ring_header(mapping.as_ptr(), max_frame_bytes)?;
    Ok(FileMapping {
        mapping,
        path: path.to_path_buf(),
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
