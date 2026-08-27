//! Pairing and trusted device storage — REQ-PICOO-PAIRING-*.

mod handshake;
mod identity;
mod store;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub use handshake::{
    new_pairing_challenge, pairing_confirm_signature, random_challenge_nonce,
    trusted_device_from_pairing, verify_pairing_confirm, PairingChallenge, PairingHandshakeError,
};
pub use identity::{DeviceIdentity, IdentityError};
use sha2::{Digest, Sha256};
pub use store::StoreError;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustedDevice {
    pub device_id: String,
    pub device_name: String,
    pub public_key: Vec<u8>,
    pub certificate_fingerprint: String,
    pub paired_at_ms: u64,
    pub last_connected_at_ms: Option<u64>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PairingError {
    #[error("public key mismatch for device {device_id}")]
    PublicKeyMismatch { device_id: String },
    #[error("device not paired")]
    NotPaired,
}

/// Derive a six-digit short code from handshake context (deterministic for tests).
pub fn derive_short_code(challenge_nonce: &[u8], local_id: &str, remote_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(challenge_nonce);
    hasher.update(local_id.as_bytes());
    hasher.update(remote_id.as_bytes());
    let digest = hasher.finalize();
    let value = u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]) % 1_000_000;
    format!("{value:06}")
}

pub fn verify_public_key(device: &TrustedDevice, observed_key: &[u8]) -> Result<(), PairingError> {
    if device.public_key == observed_key {
        Ok(())
    } else {
        Err(PairingError::PublicKeyMismatch {
            device_id: device.device_id.clone(),
        })
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TrustedDeviceStore {
    devices: HashMap<String, TrustedDevice>,
}

impl TrustedDeviceStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, device_id: &str) -> Option<&TrustedDevice> {
        self.devices.get(device_id)
    }

    pub fn list(&self) -> impl Iterator<Item = &TrustedDevice> {
        self.devices.values()
    }

    pub fn upsert(&mut self, device: TrustedDevice) {
        self.devices.insert(device.device_id.clone(), device);
    }

    pub fn remove(&mut self, device_id: &str) -> bool {
        self.devices.remove(device_id).is_some()
    }

    pub fn is_paired(&self, device_id: &str) -> bool {
        self.devices.contains_key(device_id)
    }

    pub fn verify_paired_key(
        &self,
        device_id: &str,
        observed_key: &[u8],
    ) -> Result<(), PairingError> {
        let device = self.devices.get(device_id).ok_or(PairingError::NotPaired)?;
        verify_public_key(device, observed_key)
    }

    pub fn touch_last_connected(&mut self, device_id: &str, now_ms: u64) {
        if let Some(mut device) = self.get(device_id).cloned() {
            device.last_connected_at_ms = Some(now_ms);
            self.upsert(device);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_code_is_six_digits_and_deterministic() {
        let code = derive_short_code(b"nonce", "sender", "receiver");
        assert_eq!(code.len(), 6);
        assert!(code.chars().all(|c| c.is_ascii_digit()));
        assert_eq!(code, derive_short_code(b"nonce", "sender", "receiver"));
    }

    #[test]
    fn rejects_public_key_mismatch() {
        let device = TrustedDevice {
            device_id: "d1".into(),
            device_name: "Phone".into(),
            public_key: vec![1, 2, 3],
            certificate_fingerprint: "fp".into(),
            paired_at_ms: 0,
            last_connected_at_ms: None,
        };
        assert_eq!(
            verify_public_key(&device, &[9]),
            Err(PairingError::PublicKeyMismatch {
                device_id: "d1".into()
            })
        );
    }
}
