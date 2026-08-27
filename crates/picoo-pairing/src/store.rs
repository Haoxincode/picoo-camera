//! Local trusted device persistence — REQ-PICOO-PAIRING-004, PUC-007.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{TrustedDevice, TrustedDeviceStore};

const STORE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedTrustedDevices {
    version: u32,
    devices: Vec<TrustedDevice>,
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
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let payload = PersistedTrustedDevices {
            version: STORE_VERSION,
            devices: self.list().cloned().collect(),
        };
        let json = serde_json::to_string_pretty(&payload)?;
        fs::write(path, json)?;
        Ok(())
    }
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
    Ok(store)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_save_and_load() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("trusted.json");

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
}
