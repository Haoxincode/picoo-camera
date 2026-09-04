//! Ed25519 device identity and the restricted file adapter used by Linux/tests.
//!
//! REQ-PICOO-PAIRING-007/010. Product platforms should persist the secret in
//! their OS credential store; the file adapter exists for tests and Linux tools.

use std::fs;
use std::path::Path;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{store::atomic_replace, StoreError};

const IDENTITY_ALGORITHM: &str = "Ed25519";
pub const SECRET_KEY_LEN: usize = 32;
pub const PUBLIC_KEY_LEN: usize = 32;
pub const SIGNATURE_LEN: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PersistedIdentity {
    algorithm: String,
    device_id: String,
    device_name: String,
    /// Hex encoding is only the Linux/test file-adapter representation.
    signing_key_hex: String,
    public_key_hex: String,
}

#[derive(Clone)]
pub struct DeviceIdentity {
    pub device_id: String,
    pub device_name: String,
    signing_key: SigningKey,
    public_key: [u8; PUBLIC_KEY_LEN],
}

impl std::fmt::Debug for DeviceIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DeviceIdentity")
            .field("device_id", &self.device_id)
            .field("device_name", &self.device_name)
            .field("public_key", &hex_encode(self.public_key()))
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Error)]
pub enum IdentityError {
    #[error("store: {0}")]
    Store(#[from] StoreError),
    #[error("invalid identity file: {0}")]
    Invalid(String),
    #[error("OS CSPRNG failed: {0}")]
    Random(String),
}

impl DeviceIdentity {
    pub fn generate(device_name: &str) -> Result<Self, IdentityError> {
        let mut secret = [0_u8; SECRET_KEY_LEN];
        getrandom::fill(&mut secret).map_err(|error| IdentityError::Random(error.to_string()))?;
        Ok(Self::from_signing_key(
            device_name,
            SigningKey::from_bytes(&secret),
        ))
    }

    pub fn from_secret_bytes(device_name: &str, secret: &[u8]) -> Result<Self, IdentityError> {
        let bytes: [u8; SECRET_KEY_LEN] = secret
            .try_into()
            .map_err(|_| IdentityError::Invalid("Ed25519 signing-key length".into()))?;
        Ok(Self::from_signing_key(
            device_name,
            SigningKey::from_bytes(&bytes),
        ))
    }

    fn from_signing_key(device_name: &str, signing_key: SigningKey) -> Self {
        let public_key = signing_key.verifying_key().to_bytes();
        let device_id = derive_device_id(&public_key);
        Self {
            device_id,
            device_name: device_name.to_string(),
            signing_key,
            public_key,
        }
    }

    pub fn public_key(&self) -> &[u8] {
        &self.public_key
    }

    pub fn sign(&self, message: &[u8]) -> [u8; SIGNATURE_LEN] {
        self.signing_key.sign(message).to_bytes()
    }

    /// Serialize the signing key for an OS credential-store adapter.
    ///
    /// Callers must move this value directly into protected platform storage
    /// and must never log it or persist it in an ordinary product data file.
    pub fn secret_bytes_for_secure_store(&self) -> [u8; SECRET_KEY_LEN] {
        self.signing_key.to_bytes()
    }

    pub fn set_device_name(&mut self, name: &str) {
        self.device_name = name.to_string();
    }

    pub fn load_or_create(
        path: impl AsRef<Path>,
        default_name: &str,
    ) -> Result<Self, IdentityError> {
        let path = path.as_ref();
        if path.exists() {
            Self::load_from_path(path)
        } else {
            let identity = Self::generate(default_name)?;
            identity.save_to_path(path)?;
            Ok(identity)
        }
    }

    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, IdentityError> {
        let raw = fs::read_to_string(path).map_err(StoreError::from)?;
        let persisted: PersistedIdentity = serde_json::from_str(&raw).map_err(StoreError::from)?;
        if persisted.algorithm != IDENTITY_ALGORITHM {
            return Err(IdentityError::Invalid(format!(
                "unsupported identity algorithm {}",
                persisted.algorithm
            )));
        }
        let secret = hex_decode(&persisted.signing_key_hex)
            .map_err(|e| IdentityError::Invalid(format!("signing key hex: {e}")))?;
        let public_key = hex_decode(&persisted.public_key_hex)
            .map_err(|e| IdentityError::Invalid(format!("pubkey hex: {e}")))?;
        if secret.len() != SECRET_KEY_LEN || public_key.len() != PUBLIC_KEY_LEN {
            return Err(IdentityError::Invalid("key length".into()));
        }
        let identity = Self::from_secret_bytes(&persisted.device_name, &secret)?;
        if identity.public_key() != public_key || identity.device_id != persisted.device_id {
            return Err(IdentityError::Invalid(
                "device id/public key/signing key mismatch".into(),
            ));
        }
        Ok(identity)
    }

    pub fn save_to_path(&self, path: impl AsRef<Path>) -> Result<(), IdentityError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(StoreError::from)?;
        }
        let persisted = PersistedIdentity {
            algorithm: IDENTITY_ALGORITHM.into(),
            device_id: self.device_id.clone(),
            device_name: self.device_name.clone(),
            signing_key_hex: hex_encode(&self.signing_key.to_bytes()),
            public_key_hex: hex_encode(self.public_key()),
        };
        let json = serde_json::to_string_pretty(&persisted).map_err(StoreError::from)?;
        atomic_replace(path, json.as_bytes()).map_err(StoreError::from)?;
        Ok(())
    }
}

pub fn verify_identity_signature(
    public_key: &[u8],
    message: &[u8],
    signature: &[u8],
) -> Result<(), IdentityError> {
    let public_key: [u8; PUBLIC_KEY_LEN] = public_key
        .try_into()
        .map_err(|_| IdentityError::Invalid("Ed25519 public-key length".into()))?;
    let signature = Signature::from_slice(signature)
        .map_err(|_| IdentityError::Invalid("Ed25519 signature length".into()))?;
    let verifying_key = VerifyingKey::from_bytes(&public_key)
        .map_err(|_| IdentityError::Invalid("invalid Ed25519 public key".into()))?;
    verifying_key
        .verify(message, &signature)
        .map_err(|_| IdentityError::Invalid("invalid Ed25519 signature".into()))
}

pub fn derive_device_id(public_key: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"picoo-camera device identity\0");
    hasher.update(public_key);
    let digest = hasher.finalize();
    // 128 bits keeps identifiers compact for UI/FFI while providing ample
    // collision resistance for a long-lived authorization key.
    format!("picoo-{}", hex_encode(&digest[..16]))
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
        let identity = DeviceIdentity::generate("Pixel Test").expect("generate");
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

    #[test]
    fn signatures_require_the_matching_private_key() {
        let identity = DeviceIdentity::generate("Phone").expect("identity");
        let attacker = DeviceIdentity::generate("Attacker").expect("identity");
        let message = b"channel-bound transcript";
        let signature = identity.sign(message);
        verify_identity_signature(identity.public_key(), message, &signature).expect("valid");
        assert!(verify_identity_signature(attacker.public_key(), message, &signature).is_err());
        assert!(verify_identity_signature(identity.public_key(), b"replayed", &signature).is_err());
    }

    #[test]
    fn rejects_legacy_pseudo_identity() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("identity.json");
        fs::write(
            &path,
            r#"{"version":1,"device_id":"legacy","device_name":"Phone","secret_seed_hex":"00","public_key_hex":"00"}"#,
        )
        .expect("legacy fixture");
        assert!(DeviceIdentity::load_from_path(path).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn file_adapter_restricts_identity_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("identity.json");
        DeviceIdentity::generate("Phone")
            .expect("identity")
            .save_to_path(&path)
            .expect("save");
        assert_eq!(
            fs::metadata(path).expect("metadata").permissions().mode() & 0o777,
            0o600
        );
    }
}
