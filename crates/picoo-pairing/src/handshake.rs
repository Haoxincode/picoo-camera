//! Channel-bound Ed25519 pairing transcript — REQ-PICOO-PAIRING-008/009.

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{verify_identity_signature, DeviceIdentity, TrustedDevice};

const TRANSCRIPT_DOMAIN: &[u8] = b"picoo-camera pairing transcript\0";
const SIGNATURE_DOMAIN: &[u8] = b"picoo-camera identity proof\0";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PairingHandshakeError {
    #[error("invalid identity proof signature")]
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
    fn transcript_changes_across_connection_generations() {
        let first = transcript(&[5; 32]);
        let next = PairingTranscript {
            connection_generation: first.connection_generation + 1,
            ..first
        };
        assert_ne!(first.hash().expect("first"), next.hash().expect("next"));
        assert_ne!(
            first.short_code().expect("first SAS"),
            next.short_code().expect("next SAS")
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
