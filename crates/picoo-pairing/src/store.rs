//! Local trusted device persistence — REQ-PICOO-PAIRING-005/006/010, PUC-007.

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    derive_device_id, identity::validate_identity_public_key, public_key_fingerprint,
    TrustedDevice, TrustedDeviceStore, TrustedIdentityReplacement,
};

const STORE_FORMAT: &str = "picoo-camera-ed25519-trust";
static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedTrustedDevices {
    format: String,
    devices: Vec<TrustedDevice>,
    pending_identity_replacement: Option<TrustedIdentityReplacement>,
    next_identity_replacement_revision: u64,
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported trust store format: {0}")]
    UnsupportedFormat(String),
    #[error("invalid trust store: {0}")]
    InvalidData(String),
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
            format: STORE_FORMAT.into(),
            devices: self.list().cloned().collect(),
            pending_identity_replacement: self.identity_replacement().cloned(),
            next_identity_replacement_revision: self.next_identity_replacement_revision,
        };
        validate_persisted_devices(&payload)?;
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
    if persisted.format != STORE_FORMAT {
        return Err(StoreError::UnsupportedFormat(persisted.format));
    }
    validate_persisted_devices(&persisted)?;

    let mut store = TrustedDeviceStore::new();
    for device in persisted.devices {
        store.upsert(device);
    }
    store.pending_identity_replacement = persisted.pending_identity_replacement;
    store.next_identity_replacement_revision = persisted.next_identity_replacement_revision;
    Ok(store)
}

fn validate_persisted_devices(persisted: &PersistedTrustedDevices) -> Result<(), StoreError> {
    if persisted.next_identity_replacement_revision == 0 {
        return Err(StoreError::InvalidData(
            "next identity replacement revision is invalid".into(),
        ));
    }
    let mut device_ids = HashSet::with_capacity(persisted.devices.len());
    for device in &persisted.devices {
        validate_persisted_device(device)?;
        if !device_ids.insert(device.device_id.as_str()) {
            return Err(StoreError::InvalidData(format!(
                "duplicate device id {}",
                device.device_id
            )));
        }
    }

    let Some(replacement) = persisted.pending_identity_replacement.as_ref() else {
        return Ok(());
    };
    if replacement.revision == 0
        || persisted.next_identity_replacement_revision <= replacement.revision
    {
        return Err(StoreError::InvalidData(
            "identity replacement revision is invalid".into(),
        ));
    }
    if !is_device_id(&replacement.current_device_id)
        || replacement.device_name.trim().is_empty()
        || replacement.previous_identities.is_empty()
    {
        return Err(StoreError::InvalidData(
            "identity replacement current snapshot is invalid".into(),
        ));
    }

    let mut candidate_ids = HashSet::with_capacity(replacement.previous_identities.len());
    for candidate in &replacement.previous_identities {
        if candidate.device_id == replacement.current_device_id
            || !candidate_ids.insert(candidate.device_id.as_str())
            || !is_device_id(&candidate.device_id)
            || !is_sha256_fingerprint(&candidate.certificate_fingerprint)
        {
            return Err(StoreError::InvalidData(
                "identity replacement contains an invalid candidate".into(),
            ));
        }
    }
    Ok(())
}

fn is_device_id(value: &str) -> bool {
    value
        .strip_prefix("picoo-")
        .is_some_and(|digest| digest.len() == 32 && is_lower_hex(digest))
}

fn is_sha256_fingerprint(value: &str) -> bool {
    value.len() == 64 && is_lower_hex(value)
}

fn is_lower_hex(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_persisted_device(device: &TrustedDevice) -> Result<(), StoreError> {
    validate_identity_public_key(&device.public_key)
        .map_err(|error| StoreError::InvalidData(error.to_string()))?;
    if derive_device_id(&device.public_key) != device.device_id {
        return Err(StoreError::InvalidData(format!(
            "device id does not match public key for {}",
            device.device_id
        )));
    }
    if public_key_fingerprint(&device.public_key) != device.certificate_fingerprint {
        return Err(StoreError::InvalidData(format!(
            "public-key fingerprint does not match for {}",
            device.device_id
        )));
    }
    if device.device_name.trim().is_empty() {
        return Err(StoreError::InvalidData(format!(
            "device name is empty for {}",
            device.device_id
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trusted_device(name: &str, secret_byte: u8) -> TrustedDevice {
        let identity = crate::DeviceIdentity::from_secret_bytes(name, &[secret_byte; 32])
            .expect("test identity");
        crate::trusted_device_from_pairing(
            identity.device_id(),
            identity.device_name(),
            identity.public_key(),
            100,
        )
    }

    #[test]
    fn roundtrip_save_and_load() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("trusted.json");
        fs::write(&path, b"previous contents").expect("seed existing target");

        let mut store = TrustedDeviceStore::new();
        let device = trusted_device("Pixel", 1);
        let device_id = device.device_id.clone();
        store.upsert(device);
        store.save_to_path(&path).expect("save");

        let loaded = TrustedDeviceStore::load_from_path(&path).expect("load");
        assert!(loaded.is_paired(&device_id));
        assert_eq!(
            loaded.get(&device_id).map(|d| d.device_name.as_str()),
            Some("Pixel")
        );
    }

    #[test]
    fn rejects_current_format_with_tampered_trust_record() {
        let device = trusted_device("Pixel", 2);
        let valid = PersistedTrustedDevices {
            format: STORE_FORMAT.into(),
            devices: vec![device.clone()],
            pending_identity_replacement: None,
            next_identity_replacement_revision: 1,
        };

        let mut wrong_id = valid.clone();
        wrong_id.devices[0].device_id = "picoo-tampered".into();
        assert!(matches!(
            self_from_json(&serde_json::to_string(&wrong_id).expect("json")),
            Err(StoreError::InvalidData(_))
        ));

        let mut wrong_key = valid.clone();
        wrong_key.devices[0].public_key.truncate(31);
        assert!(matches!(
            self_from_json(&serde_json::to_string(&wrong_key).expect("json")),
            Err(StoreError::InvalidData(_))
        ));

        let mut wrong_fingerprint = valid;
        wrong_fingerprint.devices[0].certificate_fingerprint = "tampered".into();
        assert!(matches!(
            self_from_json(&serde_json::to_string(&wrong_fingerprint).expect("json")),
            Err(StoreError::InvalidData(_))
        ));

        let mut zero_revision = wrong_fingerprint;
        zero_revision.devices[0].certificate_fingerprint =
            public_key_fingerprint(&zero_revision.devices[0].public_key);
        zero_revision.next_identity_replacement_revision = 0;
        assert!(matches!(
            self_from_json(&serde_json::to_string(&zero_revision).expect("json")),
            Err(StoreError::InvalidData(_))
        ));
    }

    #[test]
    fn rejects_current_format_with_missing_required_fields() {
        let raw = format!(
            r#"{{"format":"{STORE_FORMAT}","devices":[],"pending_identity_replacement":null}}"#
        );
        assert!(matches!(self_from_json(&raw), Err(StoreError::Json(_))));
    }

    #[test]
    fn rejects_structurally_invalid_identity_replacement_snapshot() {
        let current = trusted_device("Pixel", 3);
        let previous = trusted_device("Pixel", 4);
        let replacement = TrustedIdentityReplacement {
            revision: 7,
            current_device_id: current.device_id.clone(),
            device_name: current.device_name.clone(),
            previous_identities: vec![crate::TrustedIdentityCandidate {
                device_id: previous.device_id.clone(),
                certificate_fingerprint: previous.certificate_fingerprint.clone(),
                last_connected_at_ms: previous.last_connected_at_ms,
            }],
        };
        let mut persisted = PersistedTrustedDevices {
            format: STORE_FORMAT.into(),
            devices: vec![current, previous],
            pending_identity_replacement: Some(replacement),
            next_identity_replacement_revision: 8,
        };
        self_from_json(&serde_json::to_string(&persisted).expect("json")).expect("valid snapshot");

        // A frozen consent snapshot may legitimately become stale when the
        // historical device reconnects. Loading preserves it; the Receiver
        // transaction revalidates the exact record before deleting anything.
        persisted.devices[1].last_connected_at_ms = Some(999);
        self_from_json(&serde_json::to_string(&persisted).expect("json"))
            .expect("stale but structurally valid snapshot");

        persisted
            .pending_identity_replacement
            .as_mut()
            .expect("replacement")
            .previous_identities[0]
            .certificate_fingerprint = "tampered".into();
        assert!(matches!(
            self_from_json(&serde_json::to_string(&persisted).expect("json")),
            Err(StoreError::InvalidData(_))
        ));
    }

    #[test]
    fn save_rejects_invalid_in_memory_trust_record() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("trusted.json");
        let mut store = TrustedDeviceStore::new();
        let mut device = trusted_device("Pixel", 6);
        device.device_id = "picoo-tampered".into();
        store.upsert(device);

        assert!(matches!(
            store.save_to_path(&path),
            Err(StoreError::InvalidData(_))
        ));
        assert!(!path.exists());
    }

    #[test]
    fn rejects_legacy_pseudo_key_trust_store() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("trusted.json");
        fs::write(
            &path,
            r#"{"version":1,"devices":[],"pending_identity_replacement":null,"next_identity_replacement_revision":1}"#,
        )
        .expect("legacy fixture");
        assert!(TrustedDeviceStore::load_from_path(path).is_err());
    }

    #[test]
    fn failed_atomic_replace_preserves_existing_target_and_removes_temporary_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("trusted.json");
        fs::create_dir(&target).expect("directory target");
        fs::write(target.join("sentinel"), b"unchanged").expect("sentinel");

        let mut store = TrustedDeviceStore::new();
        store.upsert(trusted_device("Receiver", 5));
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
