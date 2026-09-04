//! Local trusted device persistence — REQ-PICOO-PAIRING-004, PUC-007.

use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{TrustedDevice, TrustedDeviceStore, TrustedIdentityReplacement};

const STORE_VERSION: u32 = 1;
static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedTrustedDevices {
    version: u32,
    devices: Vec<TrustedDevice>,
    #[serde(default)]
    pending_identity_replacement: Option<TrustedIdentityReplacement>,
    #[serde(default = "initial_identity_replacement_revision")]
    next_identity_replacement_revision: u64,
}

const fn initial_identity_replacement_revision() -> u64 {
    1
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported store version: {0}")]
    UnsupportedVersion(u32),
}

impl TrustedDeviceStore {
    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::new());
        }
        let raw = fs::read_to_string(path)?;
        self_from_json(&raw)
    }

    pub fn save_to_path(&self, path: impl AsRef<Path>) -> Result<(), StoreError> {
        let path = path.as_ref();
        let payload = PersistedTrustedDevices {
            version: STORE_VERSION,
            devices: self.list().cloned().collect(),
            pending_identity_replacement: self.identity_replacement().cloned(),
            next_identity_replacement_revision: self.next_identity_replacement_revision,
        };
        let json = serde_json::to_string_pretty(&payload)?;
        atomic_replace(path, json.as_bytes())?;
        Ok(())
    }
}

pub(crate) fn atomic_replace(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let file_name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "trusted store path has no file name",
        )
    })?;

    let (temporary_path, mut temporary_file) = create_temporary_file(parent, file_name)?;
    let write_result = (|| {
        temporary_file.write_all(contents)?;
        temporary_file.sync_all()?;
        drop(temporary_file);
        fs::rename(&temporary_path, path)?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary_path);
        return write_result;
    }

    // The data file is already durable before the atomic replacement. Syncing
    // the directory strengthens crash durability on Unix, but a post-rename
    // directory-sync failure cannot be rolled back safely and must not make the
    // caller restore stale in-memory trust state.
    #[cfg(unix)]
    if let Ok(directory) = fs::File::open(parent) {
        let _ = directory.sync_all();
    }
    Ok(())
}

fn create_temporary_file(
    parent: &Path,
    file_name: &std::ffi::OsStr,
) -> io::Result<(PathBuf, fs::File)> {
    for _ in 0..32 {
        let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let mut temporary_name = OsString::from(".");
        temporary_name.push(file_name);
        temporary_name.push(format!(".{}.{}.tmp", std::process::id(), sequence));
        let temporary_path = parent.join(temporary_name);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&temporary_path) {
            Ok(file) => return Ok((temporary_path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a trusted-store temporary file",
    ))
}

fn self_from_json(raw: &str) -> Result<TrustedDeviceStore, StoreError> {
    let persisted: PersistedTrustedDevices = serde_json::from_str(raw)?;
    if persisted.version != STORE_VERSION {
        return Err(StoreError::UnsupportedVersion(persisted.version));
    }
    let mut store = TrustedDeviceStore::new();
    for device in persisted.devices {
        store.upsert(device);
    }
    store.pending_identity_replacement = persisted.pending_identity_replacement;
    store.next_identity_replacement_revision = persisted.next_identity_replacement_revision.max(1);
    Ok(store)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_save_and_load() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("trusted.json");
        fs::write(&path, b"previous contents").expect("seed existing target");

        let mut store = TrustedDeviceStore::new();
        store.upsert(TrustedDevice {
            device_id: "phone-1".into(),
            device_name: "Pixel".into(),
            public_key: vec![1, 2, 3],
            certificate_fingerprint: "ab12".into(),
            paired_at_ms: 100,
            last_connected_at_ms: Some(200),
        });
        store.save_to_path(&path).expect("save");

        let loaded = TrustedDeviceStore::load_from_path(&path).expect("load");
        assert!(loaded.is_paired("phone-1"));
        assert_eq!(
            loaded.get("phone-1").map(|d| d.device_name.as_str()),
            Some("Pixel")
        );
    }

    #[test]
    fn failed_atomic_replace_preserves_existing_target_and_removes_temporary_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("trusted.json");
        fs::create_dir(&target).expect("directory target");
        fs::write(target.join("sentinel"), b"unchanged").expect("sentinel");

        let mut store = TrustedDeviceStore::new();
        store.upsert(TrustedDevice {
            device_id: "receiver".into(),
            device_name: "Receiver".into(),
            public_key: vec![1],
            certificate_fingerprint: "aa".into(),
            paired_at_ms: 0,
            last_connected_at_ms: None,
        });
        assert!(store.save_to_path(&target).is_err());
        assert_eq!(
            fs::read(target.join("sentinel")).expect("sentinel"),
            b"unchanged"
        );
        let leftovers = fs::read_dir(dir.path())
            .expect("read parent")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn remove_requires_repair_on_next_connect() {
        let mut store = TrustedDeviceStore::new();
        store.upsert(TrustedDevice {
            device_id: "phone-1".into(),
            device_name: "Pixel".into(),
            public_key: vec![1, 2, 3],
            certificate_fingerprint: "ab12".into(),
            paired_at_ms: 0,
            last_connected_at_ms: None,
        });
        assert!(store.remove("phone-1"));
        assert!(!store.is_paired("phone-1"));
    }

    #[test]
    fn clear_wipes_all_and_persists() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("trusted.json");
        let mut store = TrustedDeviceStore::new();
        store.upsert(TrustedDevice {
            device_id: "a".into(),
            device_name: "A".into(),
            public_key: vec![1],
            certificate_fingerprint: "aa".into(),
            paired_at_ms: 0,
            last_connected_at_ms: None,
        });
        store.upsert(TrustedDevice {
            device_id: "b".into(),
            device_name: "B".into(),
            public_key: vec![2],
            certificate_fingerprint: "bb".into(),
            paired_at_ms: 0,
            last_connected_at_ms: None,
        });
        assert_eq!(store.clear(), 2);
        assert!(store.is_empty());
        store.save_to_path(&path).expect("save");
        let loaded = TrustedDeviceStore::load_from_path(&path).expect("load");
        assert!(loaded.is_empty());
    }
}
