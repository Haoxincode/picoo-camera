//! Pairing and trusted device storage — REQ-PICOO-PAIRING-*.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
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
