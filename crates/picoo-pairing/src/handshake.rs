//! PairingChallenge / PairingConfirm helpers — REQ-PICOO-PAIRING-001/002.

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{derive_short_code, TrustedDevice};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingChallenge {
    pub short_code: String,
    pub challenge_nonce: Vec<u8>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PairingHandshakeError {
    #[error("invalid confirm signature")]
    InvalidSignature,
}

pub fn new_pairing_challenge(
    challenge_nonce: &[u8],
    receiver_id: &str,
    sender_id: &str,
) -> PairingChallenge {
    PairingChallenge {
        short_code: derive_short_code(challenge_nonce, receiver_id, sender_id),
        challenge_nonce: challenge_nonce.to_vec(),
    }
}

pub fn pairing_confirm_signature(
    challenge_nonce: &[u8],
    receiver_id: &str,
    sender_id: &str,
) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(challenge_nonce);
    hasher.update(receiver_id.as_bytes());
    hasher.update(sender_id.as_bytes());
    hasher.update(b"pairing-confirm-v1");
    hasher.finalize().to_vec()
}

pub fn verify_pairing_confirm(
    challenge_nonce: &[u8],
    receiver_id: &str,
    sender_id: &str,
    confirm_signature: &[u8],
) -> Result<(), PairingHandshakeError> {
    let expected = pairing_confirm_signature(challenge_nonce, receiver_id, sender_id);
    if expected == confirm_signature {
        Ok(())
    } else {
        Err(PairingHandshakeError::InvalidSignature)
    }
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Full SHA-256 hex fingerprint of a device public key (REQ-PICOO-DISCOVERY-002).
pub fn public_key_fingerprint(public_key: &[u8]) -> String {
    bytes_to_hex(&Sha256::digest(public_key))
}

/// First 8 hex chars of the public-key fingerprint for mDNS TXT.
pub fn public_key_fingerprint_prefix(public_key: &[u8]) -> String {
    let full = public_key_fingerprint(public_key);
    full.chars().take(8).collect()
}

pub fn trusted_device_from_pairing(
    sender_id: &str,
    device_name: &str,
    public_key: &[u8],
    paired_at_ms: u64,
) -> TrustedDevice {
    let fingerprint = public_key_fingerprint(public_key);
    TrustedDevice {
        device_id: sender_id.into(),
        device_name: device_name.into(),
        public_key: public_key.to_vec(),
        certificate_fingerprint: fingerprint,
        paired_at_ms,
        last_connected_at_ms: Some(paired_at_ms),
    }
}

pub fn random_challenge_nonce() -> Vec<u8> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut hasher = Sha256::new();
    hasher.update(nanos.to_le_bytes());
    hasher.update(b"picoo-pairing-nonce");
    hasher.finalize().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirm_signature_roundtrip() {
        let nonce = b"nonce-123";
        let sig = pairing_confirm_signature(nonce, "receiver", "sender");
        verify_pairing_confirm(nonce, "receiver", "sender", &sig).expect("valid");
    }

    #[test]
    fn challenge_short_code_matches_derive() {
        let nonce = b"n";
        let challenge = new_pairing_challenge(nonce, "recv", "send");
        assert_eq!(
            challenge.short_code,
            derive_short_code(nonce, "recv", "send")
        );
    }

    #[test]
    fn fingerprint_prefix_is_stable_eight_hex() {
        let key = [0xAAu8, 0xBB, 0xCC, 0xDD];
        let full = public_key_fingerprint(&key);
        let prefix = public_key_fingerprint_prefix(&key);
        assert_eq!(full.len(), 64);
        assert_eq!(prefix.len(), 8);
        assert!(prefix.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(full.starts_with(&prefix));
        assert_eq!(prefix, public_key_fingerprint_prefix(&key));
    }
}
