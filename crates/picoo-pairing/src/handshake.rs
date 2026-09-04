//! PairingChallenge / PairingConfirm helpers — REQ-PICOO-PAIRING-001/002.

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{derive_short_code, verify_identity_signature, DeviceIdentity, TrustedDevice};

const TRANSCRIPT_DOMAIN: &[u8] = b"picoo-camera pairing transcript\0";
const SIGNATURE_DOMAIN: &[u8] = b"picoo-camera identity proof\0";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingChallenge {
    pub short_code: String,
    pub challenge_nonce: Vec<u8>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PairingHandshakeError {
    #[error("invalid confirm signature")]
    InvalidSignature,
    #[error("invalid pairing transcript: {0}")]
    InvalidTranscript(String),
    #[error("OS CSPRNG failed: {0}")]
    Random(String),
}

/// Immutable facts shared by both endpoints for this exact QUIC connection.
/// Field order is role-defined, so Sender and Receiver independently serialize
/// identical bytes without relying on protobuf encoding details.
pub struct PairingTranscript<'a> {
    pub sender_id: &'a str,
    pub sender_public_key: &'a [u8],
    pub sender_nonce: &'a [u8],
    pub receiver_id: &'a str,
    pub receiver_public_key: &'a [u8],
    pub receiver_nonce: &'a [u8],
    pub channel_binding: &'a [u8],
    pub connection_generation: u64,
}

impl PairingTranscript<'_> {
    pub fn hash(&self) -> Result<[u8; 32], PairingHandshakeError> {
        for (name, value, expected) in [
            ("sender public key", self.sender_public_key, 32),
            ("sender nonce", self.sender_nonce, 32),
            ("receiver public key", self.receiver_public_key, 32),
            ("receiver nonce", self.receiver_nonce, 32),
            ("TLS exporter", self.channel_binding, 32),
        ] {
            if value.len() != expected {
                return Err(PairingHandshakeError::InvalidTranscript(format!(
                    "{name} must contain {expected} bytes"
                )));
            }
        }
        if self.sender_id.is_empty()
            || self.receiver_id.is_empty()
            || self.connection_generation == 0
        {
            return Err(PairingHandshakeError::InvalidTranscript(
                "device ids and connection generation must be present".into(),
            ));
        }

        let mut encoded = Vec::with_capacity(256);
        encoded.extend_from_slice(TRANSCRIPT_DOMAIN);
        append_field(&mut encoded, self.sender_id.as_bytes());
        append_field(&mut encoded, self.sender_public_key);
        append_field(&mut encoded, self.sender_nonce);
        append_field(&mut encoded, self.receiver_id.as_bytes());
        append_field(&mut encoded, self.receiver_public_key);
        append_field(&mut encoded, self.receiver_nonce);
        append_field(&mut encoded, self.channel_binding);
        encoded.extend_from_slice(&self.connection_generation.to_be_bytes());
        Ok(Sha256::digest(encoded).into())
    }

    pub fn short_code(&self) -> Result<String, PairingHandshakeError> {
        let digest = self.hash()?;
        let value = u32::from_be_bytes(digest[..4].try_into().expect("four-byte digest prefix"))
            % 1_000_000;
        Ok(format!("{value:06}"))
    }
}

fn append_field(output: &mut Vec<u8>, value: &[u8]) {
    let length = u32::try_from(value.len()).expect("pairing field length fits u32");
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(value);
}

pub fn sign_transcript_phase(
    identity: &DeviceIdentity,
    transcript_hash: &[u8; 32],
    phase: &[u8],
) -> [u8; 64] {
    identity.sign(&transcript_phase_message(transcript_hash, phase))
}

pub fn verify_transcript_phase(
    public_key: &[u8],
    transcript_hash: &[u8; 32],
    phase: &[u8],
    signature: &[u8],
) -> Result<(), PairingHandshakeError> {
    verify_identity_signature(
        public_key,
        &transcript_phase_message(transcript_hash, phase),
        signature,
    )
    .map_err(|_| PairingHandshakeError::InvalidSignature)
}

fn transcript_phase_message(transcript_hash: &[u8; 32], phase: &[u8]) -> Vec<u8> {
    let mut message = Vec::with_capacity(SIGNATURE_DOMAIN.len() + 4 + phase.len() + 32);
    message.extend_from_slice(SIGNATURE_DOMAIN);
    append_field(&mut message, phase);
    message.extend_from_slice(transcript_hash);
    message
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

/// Bind a pairing control phase to the active challenge and both device identities.
/// QUIC authenticates the channel; this transcript hash prevents stale-session messages
/// from being accepted for a later challenge on the same process.
pub fn pairing_transcript_hash(
    challenge_nonce: &[u8],
    receiver_id: &str,
    sender_id: &str,
    phase: &[u8],
) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(challenge_nonce);
    hasher.update(receiver_id.as_bytes());
    hasher.update(sender_id.as_bytes());
    hasher.update(phase);
    hasher.finalize().to_vec()
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

pub fn random_challenge_nonce() -> Result<[u8; 32], PairingHandshakeError> {
    let mut nonce = [0_u8; 32];
    getrandom::fill(&mut nonce)
        .map_err(|error| PairingHandshakeError::Random(error.to_string()))?;
    Ok(nonce)
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
    fn pairing_phase_transcripts_are_challenge_and_phase_bound() {
        let approval = pairing_transcript_hash(b"nonce-a", "receiver", "sender", b"approval-v2");
        let another_challenge =
            pairing_transcript_hash(b"nonce-b", "receiver", "sender", b"approval-v2");
        let commit = pairing_transcript_hash(b"nonce-a", "receiver", "sender", b"commit-v2");

        assert_ne!(approval, another_challenge);
        assert_ne!(approval, commit);
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
    fn challenge_nonce_comes_from_os_csprng() {
        let first = random_challenge_nonce().expect("nonce");
        let second = random_challenge_nonce().expect("nonce");
        assert_ne!(first, second);
        assert_ne!(first, [0; 32]);
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

    fn transcript<'a>(binding: &'a [u8]) -> PairingTranscript<'a> {
        PairingTranscript {
            sender_id: "picoo-sender",
            sender_public_key: &[1; 32],
            sender_nonce: &[2; 32],
            receiver_id: "picoo-receiver",
            receiver_public_key: &[3; 32],
            receiver_nonce: &[4; 32],
            channel_binding: binding,
            connection_generation: 9,
        }
    }

    #[test]
    fn sas_is_independently_deterministic_and_channel_bound() {
        let first = transcript(&[5; 32]);
        let same = transcript(&[5; 32]);
        let mitm_channel = transcript(&[6; 32]);
        assert_eq!(first.hash().expect("hash"), same.hash().expect("hash"));
        assert_eq!(
            first.short_code().expect("sas"),
            same.short_code().expect("sas")
        );
        assert_ne!(
            first.short_code().expect("sas"),
            mitm_channel.short_code().expect("sas")
        );
    }

    #[test]
    fn signatures_bind_phase_transcript_and_private_key() {
        let sender = DeviceIdentity::generate("Sender").expect("sender");
        let attacker = DeviceIdentity::generate("Attacker").expect("attacker");
        let hash = transcript(&[5; 32]).hash().expect("hash");
        let signature = sign_transcript_phase(&sender, &hash, b"pairing-confirm");
        verify_transcript_phase(sender.public_key(), &hash, b"pairing-confirm", &signature)
            .expect("valid");
        assert!(verify_transcript_phase(
            attacker.public_key(),
            &hash,
            b"pairing-confirm",
            &signature
        )
        .is_err());
        assert!(verify_transcript_phase(
            sender.public_key(),
            &hash,
            b"pairing-approval",
            &signature
        )
        .is_err());
    }
}
