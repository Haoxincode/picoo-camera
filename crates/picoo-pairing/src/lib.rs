//! Pairing and trusted device storage — REQ-PICOO-PAIRING-*.

mod handshake;
mod identity;
mod store;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub use handshake::{
    public_key_fingerprint, public_key_fingerprint_prefix, random_challenge_nonce,
    sign_transcript_phase, trusted_device_from_pairing, verify_transcript_phase,
    PairingHandshakeError, PairingTranscript,
};
pub use identity::{
    derive_device_id, verify_identity_signature, DeviceIdentity, IdentityError, PUBLIC_KEY_LEN,
    SECRET_KEY_LEN, SIGNATURE_LEN,
};
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

/// One historical trust credential shown to the user before it can be revoked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustedIdentityCandidate {
    pub device_id: String,
    pub certificate_fingerprint: String,
    pub last_connected_at_ms: Option<u64>,
}

/// An immutable post-pairing cleanup decision. Persisting the exact snapshot
/// prevents a restart from losing the prompt or widening the user's consent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustedIdentityReplacement {
    pub revision: u64,
    pub current_device_id: String,
    pub device_name: String,
    pub previous_identities: Vec<TrustedIdentityCandidate>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PairingError {
    #[error("public key mismatch for device {device_id}")]
    PublicKeyMismatch { device_id: String },
    #[error("device not paired")]
    NotPaired,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustedDeviceStore {
    devices: HashMap<String, TrustedDevice>,
    pending_identity_replacement: Option<TrustedIdentityReplacement>,
    next_identity_replacement_revision: u64,
    /// Process-local observation revision. Persistence has its own schema and
    /// must not turn this cache-invalidation detail into stored trust state.
    #[serde(skip, default = "initial_store_revision")]
    revision: u64,
}

const fn initial_store_revision() -> u64 {
    1
}

impl Default for TrustedDeviceStore {
    fn default() -> Self {
        Self {
            devices: HashMap::new(),
            pending_identity_replacement: None,
            next_identity_replacement_revision: 1,
            revision: initial_store_revision(),
        }
    }
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
        if self.devices.get(&device.device_id) == Some(&device) {
            return;
        }
        self.devices.insert(device.device_id.clone(), device);
        self.bump_revision();
    }

    pub fn remove(&mut self, device_id: &str) -> bool {
        let removed = self.devices.remove(device_id).is_some();
        if removed {
            self.bump_revision();
        }
        removed
    }

    /// Remove every trusted device (REQ-PICOO-PAIRING-005 / PUC-007 wipe).
    pub fn clear(&mut self) -> usize {
        let n = self.devices.len();
        self.devices.clear();
        if n > 0 {
            self.bump_revision();
        }
        n
    }

    /// Monotonic process-local revision for immutable presentation caches.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn len(&self) -> usize {
        self.devices.len()
    }

    pub fn is_empty(&self) -> bool {
        self.devices.is_empty()
    }

    pub fn is_paired(&self, device_id: &str) -> bool {
        self.devices.contains_key(device_id)
    }

    /// Trusted identities with the same user-visible name as `device_id`.
    ///
    /// A display name is never a trust key. This query only prepares an
    /// explicit, post-pairing replacement decision (REQ-PICOO-PAIRING-006).
    pub fn same_name_identity_ids(&self, device_id: &str) -> Vec<String> {
        let Some(current) = self.devices.get(device_id) else {
            return Vec::new();
        };
        let current_name = normalized_device_name(&current.device_name);
        if current_name.is_empty() {
            return Vec::new();
        }

        let mut ids = self
            .devices
            .values()
            .filter(|device| {
                device.device_id != device_id
                    && normalized_device_name(&device.device_name) == current_name
            })
            .map(|device| device.device_id.clone())
            .collect::<Vec<_>>();
        ids.sort();
        ids
    }

    pub fn identity_replacement(&self) -> Option<&TrustedIdentityReplacement> {
        self.pending_identity_replacement.as_ref()
    }

    pub fn allocate_identity_replacement_revision(&mut self) -> u64 {
        let revision = self.next_identity_replacement_revision.max(1);
        self.next_identity_replacement_revision = revision.saturating_add(1);
        revision
    }

    pub fn set_identity_replacement(&mut self, replacement: Option<TrustedIdentityReplacement>) {
        if self.pending_identity_replacement == replacement {
            return;
        }
        self.pending_identity_replacement = replacement;
        self.bump_revision();
    }

    pub fn dismiss_identity_replacement(&mut self, revision: u64) -> bool {
        if self
            .pending_identity_replacement
            .as_ref()
            .is_some_and(|replacement| replacement.revision == revision)
        {
            self.pending_identity_replacement = None;
            self.bump_revision();
            true
        } else {
            false
        }
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

    fn bump_revision(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }
}

fn normalized_device_name(name: &str) -> String {
    name.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn same_name_identities_are_candidates_but_remain_distinct_trust_keys() {
        let mut store = TrustedDeviceStore::new();
        for (device_id, device_name) in [
            ("current", "  Pixel 9 Pro "),
            ("older-b", "pixel 9 pro"),
            ("older-a", "PIXEL 9 PRO"),
            ("other", "iPhone"),
        ] {
            store.upsert(TrustedDevice {
                device_id: device_id.into(),
                device_name: device_name.into(),
                public_key: vec![device_id.len() as u8],
                certificate_fingerprint: device_id.into(),
                paired_at_ms: 0,
                last_connected_at_ms: None,
            });
        }

        assert_eq!(
            store.same_name_identity_ids("current"),
            vec!["older-a".to_string(), "older-b".to_string()]
        );
        assert!(store.is_paired("current"));
        assert!(store.is_paired("older-a"));
    }

    #[test]
    fn observation_revision_changes_only_when_visible_trust_state_changes() {
        let mut store = TrustedDeviceStore::new();
        let initial = store.revision();
        let device = TrustedDevice {
            device_id: "current".into(),
            device_name: "Phone".into(),
            public_key: vec![1],
            certificate_fingerprint: "fingerprint".into(),
            paired_at_ms: 1,
            last_connected_at_ms: None,
        };

        store.upsert(device.clone());
        let inserted = store.revision();
        assert!(inserted > initial);
        store.upsert(device);
        assert_eq!(store.revision(), inserted);

        assert!(!store.remove("missing"));
        assert_eq!(store.revision(), inserted);
        assert!(store.remove("current"));
        assert!(store.revision() > inserted);
    }
}
