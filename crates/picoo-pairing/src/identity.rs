//! Durable local device identity — REQ-PICOO-PAIRING-001 / PUC-001.
//!
//! Generates a stable public key (derived from a random seed) and device_id so
//! ClientHello no longer uses hard-coded stub bytes on Android.

use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::StoreError;

const IDENTITY_VERSION: u32 = 1;
const SEED_LEN: usize = 32;
const PUBLIC_KEY_LEN: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PersistedIdentity {
    version: u32,
    device_id: String,
    device_name: String,
    /// Hex-encoded seed (never sent on the wire).
    secret_seed_hex: String,
    /// Hex-encoded public key material advertised in ClientHello.
    public_key_hex: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceIdentity {
    pub device_id: String,
    pub device_name: String,
    secret_seed: Vec<u8>,
    public_key: Vec<u8>,
}

#[derive(Debug, Error)]
pub enum IdentityError {
    #[error("store: {0}")]
    Store(#[from] StoreError),
    #[error("invalid identity file: {0}")]
    Invalid(String),
}

impl DeviceIdentity {
    pub fn generate(device_name: &str) -> Self {
        let seed = random_seed();
        let public_key = derive_public_key(&seed);
        let device_id = derive_device_id(&public_key);
        Self {
            device_id,
            device_name: device_name.to_string(),
            secret_seed: seed,
            public_key,
        }
    }

    pub fn public_key(&self) -> &[u8] {
        &self.public_key
    }

    pub fn set_device_name(&mut self, name: &str) {
        self.device_name = name.to_string();
    }

    pub fn load_or_create(path: impl AsRef<Path>, default_name: &str) -> Result<Self, IdentityError> {
        let path = path.as_ref();
        if path.exists() {
            Self::load_from_path(path)
        } else {
            let identity = Self::generate(default_name);
            identity.save_to_path(path)?;
            Ok(identity)
        }
    }

    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, IdentityError> {
        let raw = fs::read_to_string(path).map_err(StoreError::from)?;
        let persisted: PersistedIdentity =
            serde_json::from_str(&raw).map_err(StoreError::from)?;
        if persisted.version != IDENTITY_VERSION {
            return Err(IdentityError::Invalid(format!(
                "unsupported version {}",
                persisted.version
            )));
        }
        let secret_seed = hex_decode(&persisted.secret_seed_hex)
            .map_err(|e| IdentityError::Invalid(format!("seed hex: {e}")))?;
        let public_key = hex_decode(&persisted.public_key_hex)
            .map_err(|e| IdentityError::Invalid(format!("pubkey hex: {e}")))?;
        if secret_seed.len() != SEED_LEN || public_key.len() != PUBLIC_KEY_LEN {
            return Err(IdentityError::Invalid("key length".into()));
        }
        if derive_public_key(&secret_seed) != public_key {
            return Err(IdentityError::Invalid("pubkey/seed mismatch".into()));
        }
        Ok(Self {
            device_id: persisted.device_id,
            device_name: persisted.device_name,
            secret_seed,
            public_key,
        })
    }

    pub fn save_to_path(&self, path: impl AsRef<Path>) -> Result<(), IdentityError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(StoreError::from)?;
        }
        let persisted = PersistedIdentity {
            version: IDENTITY_VERSION,
            device_id: self.device_id.clone(),
            device_name: self.device_name.clone(),
            secret_seed_hex: hex_encode(&self.secret_seed),
            public_key_hex: hex_encode(&self.public_key),
        };
        let json = serde_json::to_string_pretty(&persisted).map_err(StoreError::from)?;
        fs::write(path, json).map_err(StoreError::from)?;
        Ok(())
    }
}

fn random_seed() -> Vec<u8> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut hasher = Sha256::new();
    hasher.update(nanos.to_le_bytes());
    hasher.update(b"picoo-device-seed-v1");
    // Mix in another clock sample to reduce collision in rapid tests.
    let nanos2 = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    hasher.update(nanos2.to_le_bytes());
    hasher.finalize().to_vec()
}

fn derive_public_key(seed: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(seed);
    hasher.update(b"picoo-device-pubkey-v1");
    hasher.finalize().to_vec()
}

fn derive_device_id(public_key: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(public_key);
    hasher.update(b"picoo-device-id-v1");
    let digest = hasher.finalize();
    format!("picoo-{}", hex_encode(&digest[..8]))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_decode(hex: &str) -> Result<Vec<u8>, String> {
    if !hex.len().is_multiple_of(2) {
        return Err("odd length".into());
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|e| e.to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_stable_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("identity.json");
        let identity = DeviceIdentity::generate("Pixel Test");
        assert!(identity.device_id.starts_with("picoo-"));
        assert_eq!(identity.public_key().len(), PUBLIC_KEY_LEN);
        identity.save_to_path(&path).expect("save");

        let loaded = DeviceIdentity::load_from_path(&path).expect("load");
        assert_eq!(loaded.device_id, identity.device_id);
        assert_eq!(loaded.public_key(), identity.public_key());
        assert_eq!(loaded.device_name, "Pixel Test");
    }

    #[test]
    fn load_or_create_is_idempotent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("identity.json");
        let first = DeviceIdentity::load_or_create(&path, "Phone").expect("create");
        let second = DeviceIdentity::load_or_create(&path, "Ignored").expect("load");
        assert_eq!(first.device_id, second.device_id);
        assert_eq!(first.public_key(), second.public_key());
        assert_eq!(second.device_name, "Phone");
    }
}
