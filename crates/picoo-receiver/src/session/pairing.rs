//! Pairing handshake: ClientHello, short-code confirm, commit, trusted store.
//!
//! REQ-PICOO-PAIRING-*

use std::path::Path;
use std::time::{Duration, Instant};

use picoo_pairing::{
    derive_device_id, random_challenge_nonce, sign_transcript_phase, trusted_device_from_pairing,
    verify_transcript_phase, PairingTranscript, TrustedDeviceStore,
};
use picoo_protocol::control::{
    control_envelope::Payload as ControlPayload, ClientHello, PairingApproval, PairingCommit,
    PairingComplete, PairingConfirm, ServerHello, SessionError,
};
use picoo_session::{StreamState, TrustState};
use picoo_transport::SessionId;

use super::{
    ReceiverCloseReason, ReceiverEvent, ReceiverSession, TrustedIdentityCandidate,
    TrustedIdentityReplacement,
};
use crate::{ReceiverError, PAIRING_CHALLENGE_TTL};

pub(in crate::session) struct ActiveSender {
    pub(in crate::session) sender_id: String,
    pub(in crate::session) device_name: String,
    pub(in crate::session) public_key: Vec<u8>,
    pub(in crate::session) video_allowed: bool,
}

pub(in crate::session) struct PendingPairing {
    pub(in crate::session) session: SessionId,
    pub(in crate::session) transcript_hash: [u8; 32],
    pub(in crate::session) short_code: String,
    pub(in crate::session) pairing_required: bool,
    pub(in crate::session) local_confirmed: bool,
    pub(in crate::session) remote_confirmed: bool,
    pub(in crate::session) approval_sent: bool,
    pub(in crate::session) sender_committed: bool,
    pub(in crate::session) receiver_committed: bool,
    /// PUC-001 / AC-M-PAIR-02: challenge valid for 60s (wall clock).
    pub(in crate::session) expires_at: Instant,
}

pub(in crate::session) const SERVER_HELLO_PHASE: &[u8] = b"server-hello";
pub(in crate::session) const SENDER_CONFIRM_PHASE: &[u8] = b"sender-confirm";
pub(in crate::session) const PAIRING_APPROVAL_PHASE: &[u8] = b"receiver-approval";
pub(in crate::session) const PAIRING_COMMIT_PHASE: &[u8] = b"sender-commit";
pub(in crate::session) const PAIRING_COMPLETE_PHASE: &[u8] = b"receiver-complete";

impl ReceiverSession {
    pub fn with_trusted_store(self, path: impl AsRef<Path>) -> Result<Self, ReceiverError> {
        let path = path.as_ref().to_path_buf();
        let store = TrustedDeviceStore::load_from_path(&path)?;
        Ok(self.with_loaded_trusted_store(store, path))
    }

    /// Attach a trust snapshot that was validated before media/transport
    /// runtime construction. Desktop startup uses this to fail closed before
    /// creating those resources when the store is corrupt.
    pub fn with_loaded_trusted_store(
        mut self,
        store: TrustedDeviceStore,
        path: impl AsRef<Path>,
    ) -> Self {
        self.trusted = store;
        self.trusted_store_path = Some(path.as_ref().to_path_buf());
        self
    }

    pub fn trusted_store_path(&self) -> Option<&Path> {
        self.trusted_store_path.as_deref()
    }

    pub fn remove_trusted_device(&mut self, device_id: &str) -> Result<bool, ReceiverError> {
        let previous = self.trusted.clone();
        let removed = self.trusted.remove(device_id);
        if removed {
            if let Err(error) = self.persist_trusted() {
                self.trusted = previous;
                return Err(error);
            }
        }
        Ok(removed)
    }

    /// One immutable decision emitted only when a previously unknown Sender
    /// completes PairingCommit. Trusted reconnects never synthesize it.
    pub fn trusted_identity_replacement(&self) -> Option<&TrustedIdentityReplacement> {
        self.trusted.identity_replacement()
    }

    /// Keep the newly paired identity and transactionally revoke exactly the
    /// identities shown for this decision. A persistence failure restores all
    /// entries and keeps the decision retryable.
    pub fn replace_trusted_identity_history(
        &mut self,
        revision: u64,
    ) -> Result<usize, ReceiverError> {
        let replacement = self
            .trusted
            .identity_replacement()
            .filter(|replacement| replacement.revision == revision)
            .cloned()
            .ok_or(ReceiverError::StaleTrustedIdentityReplacement)?;
        if !self.trusted.is_paired(&replacement.current_device_id)
            || replacement.previous_identities.iter().any(|candidate| {
                self.trusted.get(&candidate.device_id).is_none_or(|device| {
                    device.certificate_fingerprint != candidate.certificate_fingerprint
                        || device.last_connected_at_ms != candidate.last_connected_at_ms
                })
            })
        {
            return Err(ReceiverError::StaleTrustedIdentityReplacement);
        }
        let previous = self.trusted.clone();
        let mut removed = 0;
        for candidate in &replacement.previous_identities {
            removed += usize::from(self.trusted.remove(&candidate.device_id));
        }
        self.trusted.set_identity_replacement(None);
        if let Err(error) = self.persist_trusted() {
            self.trusted = previous;
            return Err(error);
        }
        Ok(removed)
    }

    pub fn dismiss_trusted_identity_replacement(
        &mut self,
        revision: u64,
    ) -> Result<bool, ReceiverError> {
        let previous = self.trusted.clone();
        let dismissed = self.trusted.dismiss_identity_replacement(revision);
        if dismissed {
            if let Err(error) = self.persist_trusted() {
                self.trusted = previous;
                return Err(error);
            }
        }
        Ok(dismissed)
    }

    pub(crate) fn prepare_trusted_identity_replacement(&mut self, current_device_id: &str) {
        self.trusted.set_identity_replacement(None);
        let Some(current) = self.trusted.get(current_device_id) else {
            return;
        };
        let current_device_id = current.device_id.clone();
        let current_device_name = current.device_name.clone();
        let mut previous_identities = self
            .trusted
            .same_name_identity_ids(&current_device_id)
            .into_iter()
            .filter_map(|device_id| self.trusted.get(&device_id))
            .map(|device| TrustedIdentityCandidate {
                device_id: device.device_id.clone(),
                certificate_fingerprint: device.certificate_fingerprint.clone(),
                last_connected_at_ms: device.last_connected_at_ms,
            })
            .collect::<Vec<_>>();
        previous_identities.sort_by(|left, right| {
            right
                .last_connected_at_ms
                .cmp(&left.last_connected_at_ms)
                .then_with(|| left.device_id.cmp(&right.device_id))
        });
        if previous_identities.is_empty() {
            return;
        }
        let revision = self.trusted.allocate_identity_replacement_revision();
        self.trusted
            .set_identity_replacement(Some(TrustedIdentityReplacement {
                revision,
                current_device_id,
                device_name: current_device_name,
                previous_identities,
            }));
    }

    /// Wipe all trusted devices; subsequent connects require re-pairing (PUC-007).
    pub fn clear_trusted_devices(&mut self) -> Result<usize, ReceiverError> {
        let previous = self.trusted.clone();
        let n = self.trusted.clear();
        if n > 0 {
            if let Err(error) = self.persist_trusted() {
                self.trusted = previous;
                return Err(error);
            }
        }
        Ok(n)
    }

    fn persist_trusted(&self) -> Result<(), ReceiverError> {
        if let Some(path) = &self.trusted_store_path {
            self.trusted.save_to_path(path)?;
        }
        Ok(())
    }

    pub fn trusted_devices(&self) -> &TrustedDeviceStore {
        &self.trusted
    }

    pub fn trusted_devices_mut(&mut self) -> &mut TrustedDeviceStore {
        &mut self.trusted
    }

    /// Auto-accept already-trusted senders without short-code (REQ-PICOO-UI-002 / PRD §16).
    pub fn set_auto_accept_paired(&mut self, enabled: bool) {
        self.auto_accept_paired = enabled;
    }

    pub fn auto_accept_paired(&self) -> bool {
        self.auto_accept_paired
    }

    pub fn pairing_required(&self) -> bool {
        self.pending_pairing
            .as_ref()
            .is_some_and(|pairing| pairing.pairing_required)
    }

    pub fn pairing_short_code(&self) -> Option<&str> {
        self.pending_pairing.as_ref().and_then(|pairing| {
            (!pairing.short_code.is_empty()).then_some(pairing.short_code.as_str())
        })
    }

    /// Remaining TTL for the active pairing challenge, if any.
    pub fn pairing_ttl_remaining(&self) -> Option<Duration> {
        let pending = self.pending_pairing.as_ref()?;
        Some(pending.expires_at.saturating_duration_since(Instant::now()))
    }

    /// Drop expired pending pairing (clears short code / modal).
    pub fn expire_pending_pairing_if_needed(&mut self) {
        let Some(pending) = self.pending_pairing.as_ref() else {
            return;
        };
        if Instant::now() < pending.expires_at {
            return;
        }
        let session = pending.session;
        let _ = self.apply_receiver_event(ReceiverEvent::AbortConnection {
            generation: session.0,
            reason: ReceiverCloseReason::PairingExpired,
        });
    }

    /// User confirmed the six-digit code on desktop (PUC-001).
    pub fn confirm_pairing_locally(&mut self) -> Result<(), ReceiverError> {
        self.expire_pending_pairing_if_needed();
        if let Some(pending) = self.pending_pairing.as_mut() {
            pending.local_confirmed = true;
        }
        self.advance_pairing()
    }

    /// User explicitly rejected the active short-code challenge on desktop.
    ///
    /// The reliable SessionError lets mobile distinguish an intentional reject
    /// from an unrelated transport interruption (REQ-PICOO-PAIRING-001 /
    /// AC-M-PAIR-03).
    pub fn reject_pairing_locally(&mut self) -> Result<(), ReceiverError> {
        self.expire_pending_pairing_if_needed();
        let Some(pending) = self.pending_pairing.as_ref() else {
            return Ok(());
        };
        let session = pending.session;
        let error = SessionError {
            code: "PAIRING_REJECTED".into(),
            message: "desktop user rejected the pairing challenge".into(),
        };
        self.send_control_payload(session, ControlPayload::SessionError(error))?;
        self.apply_receiver_event(ReceiverEvent::AbortConnection {
            generation: session.0,
            reason: ReceiverCloseReason::PairingRejected,
        })?;
        Ok(())
    }

    pub fn is_awaiting_pairing_confirm(&self) -> bool {
        self.pending_pairing.is_some()
    }

    /// Test hook: expire the pending pairing challenge immediately.
    #[cfg(test)]
    pub fn force_expire_pending_pairing_for_test(&mut self) {
        if let Some(pending) = self.pending_pairing.as_mut() {
            pending.expires_at = Instant::now() - Duration::from_millis(1);
        }
        self.expire_pending_pairing_if_needed();
    }

    pub(crate) fn handle_client_hello(
        &mut self,
        session: SessionId,
        hello: ClientHello,
    ) -> Result<(), ReceiverError> {
        if hello.sender_id.is_empty()
            || derive_device_id(&hello.public_key) != hello.sender_id
            || hello.sender_nonce.len() != 32
        {
            return Err(ReceiverError::Protocol(
                "ClientHello contains an invalid Sender identity".into(),
            ));
        }
        // PAIRING-004 / PUC-007: known device_id with changed public key → hard reject
        // (no pending re-pair, trust store unchanged; peer must remove + re-pair).
        if self.trusted.is_paired(&hello.sender_id)
            && self
                .trusted
                .verify_paired_key(&hello.sender_id, &hello.public_key)
                .is_err()
        {
            let err = SessionError {
                code: "PUBLIC_KEY_CHANGED".into(),
                message: "paired device public key changed; remove and re-pair".into(),
            };
            let _ = self.send_control_payload(session, ControlPayload::SessionError(err));
            self.apply_receiver_event(ReceiverEvent::AbortConnection {
                generation: session.0,
                reason: ReceiverCloseReason::PublicKeyChanged,
            })?;
            return Ok(());
        }

        let paired = self
            .trusted
            .verify_paired_key(&hello.sender_id, &hello.public_key)
            .is_ok();
        let auto_accept = paired && self.auto_accept_paired;
        let receiver_nonce =
            random_challenge_nonce().map_err(|error| ReceiverError::Protocol(error.to_string()))?;
        let channel_binding = self.transport.channel_binding(session)?;
        let connection_generation = self.control_generation.ok_or_else(|| {
            ReceiverError::Protocol("missing control connection generation".into())
        })?;
        let transcript = PairingTranscript {
            sender_id: &hello.sender_id,
            sender_public_key: &hello.public_key,
            sender_nonce: &hello.sender_nonce,
            receiver_id: self.identity.receiver_id(),
            receiver_public_key: self.identity.public_key(),
            receiver_nonce: &receiver_nonce,
            channel_binding: &channel_binding,
            connection_generation,
        };
        let transcript_hash = transcript
            .hash()
            .map_err(|error| ReceiverError::Protocol(error.to_string()))?;
        let short_code = transcript
            .short_code()
            .map_err(|error| ReceiverError::Protocol(error.to_string()))?;

        let server_hello = ServerHello {
            receiver_id: self.identity.receiver_id().to_owned(),
            display_name: self.identity.display_name().to_owned(),
            public_key: self.identity.public_key().to_vec(),
            pairing_required: !auto_accept,
            receiver_nonce: receiver_nonce.to_vec(),
            transcript_hash: transcript_hash.to_vec(),
            identity_signature: sign_transcript_phase(
                self.identity.signer(),
                &transcript_hash,
                SERVER_HELLO_PHASE,
            )
            .to_vec(),
        };
        self.send_control_payload(session, ControlPayload::ServerHello(server_hello))?;

        self.pending_pairing = Some(PendingPairing {
            session,
            transcript_hash,
            short_code: if auto_accept {
                String::new()
            } else {
                short_code
            },
            pairing_required: !auto_accept,
            local_confirmed: auto_accept,
            remote_confirmed: false,
            approval_sent: false,
            sender_committed: false,
            receiver_committed: false,
            expires_at: Instant::now() + PAIRING_CHALLENGE_TTL,
        });
        self.active_sender = Some(ActiveSender {
            sender_id: hello.sender_id,
            device_name: hello.device_name,
            public_key: hello.public_key,
            video_allowed: false,
        });
        self.lifecycle.runtime.set_trust(if auto_accept {
            TrustState::Unknown
        } else {
            TrustState::Pairing
        });
        self.lifecycle.runtime.set_stream(StreamState::Negotiating);
        Ok(())
    }

    pub(crate) fn handle_pairing_confirm(
        &mut self,
        session: SessionId,
        confirm: PairingConfirm,
    ) -> Result<(), ReceiverError> {
        let pending = self
            .pending_pairing
            .as_ref()
            .ok_or_else(|| ReceiverError::Protocol("no pending pairing".into()))?;

        if Instant::now() >= pending.expires_at {
            self.pending_pairing = None;
            self.lifecycle.runtime.set_trust(TrustState::Unknown);
            self.lifecycle.runtime.set_stream(StreamState::Idle);
            return Err(ReceiverError::Protocol("pairing challenge expired".into()));
        }

        if session != pending.session {
            return Err(ReceiverError::Protocol("pairing session mismatch".into()));
        }

        let sender = self
            .active_sender
            .as_ref()
            .ok_or_else(|| ReceiverError::Protocol("missing active Sender".into()))?;

        if confirm.transcript_hash != pending.transcript_hash {
            return Err(ReceiverError::Protocol(
                "PairingConfirm transcript mismatch".into(),
            ));
        }
        verify_transcript_phase(
            &sender.public_key,
            &pending.transcript_hash,
            SENDER_CONFIRM_PHASE,
            &confirm.identity_signature,
        )
        .map_err(|error| ReceiverError::Protocol(error.to_string()))?;

        if let Some(pending) = self.pending_pairing.as_mut() {
            pending.remote_confirmed = true;
        }
        self.advance_pairing()
    }

    pub(crate) fn verify_sender_phase(
        &self,
        session: SessionId,
        transcript_hash: &[u8],
        signature: &[u8],
        phase: &[u8],
    ) -> bool {
        let Some(pending) = self.pending_pairing.as_ref() else {
            return false;
        };
        let Some(active) = self.active_sender.as_ref() else {
            return false;
        };
        session == pending.session
            && transcript_hash == pending.transcript_hash
            && verify_transcript_phase(
                &active.public_key,
                &pending.transcript_hash,
                phase,
                signature,
            )
            .is_ok()
    }

    fn finish_pairing_commit(&mut self) -> Result<(), ReceiverError> {
        if let Some(pending) = self.pending_pairing.as_mut() {
            pending.sender_committed = true;
        }
        self.advance_pairing()
    }

    pub(crate) fn handle_pairing_commit(
        &mut self,
        session: SessionId,
        commit: PairingCommit,
    ) -> Result<(), ReceiverError> {
        if self.verify_sender_phase(
            session,
            &commit.transcript_hash,
            &commit.identity_signature,
            PAIRING_COMMIT_PHASE,
        ) {
            self.finish_pairing_commit()
        } else {
            Err(ReceiverError::Protocol(
                "PairingCommit identity proof is invalid".into(),
            ))
        }
    }

    fn advance_pairing(&mut self) -> Result<(), ReceiverError> {
        let Some(pending) = self.pending_pairing.as_ref() else {
            return Ok(());
        };
        if !pending.local_confirmed || !pending.remote_confirmed {
            return Ok(());
        }
        let session = pending.session;
        let transcript_hash = pending.transcript_hash;
        let approval_sent = pending.approval_sent;
        let sender_committed = pending.sender_committed;
        let receiver_committed = pending.receiver_committed;

        if !sender_committed {
            if approval_sent {
                return Ok(());
            }
            let approval = PairingApproval {
                transcript_hash: transcript_hash.to_vec(),
                identity_signature: sign_transcript_phase(
                    self.identity.signer(),
                    &transcript_hash,
                    PAIRING_APPROVAL_PHASE,
                )
                .to_vec(),
            };
            self.send_control_payload(session, ControlPayload::PairingApproval(approval))?;
            if let Some(pending) = self.pending_pairing.as_mut() {
                pending.approval_sent = true;
            }
            return Ok(());
        }

        if !receiver_committed {
            let now_ms = self.now_ms();
            let active = self.active_sender.as_ref().expect("active sender");
            let active_sender_id = active.sender_id.clone();
            let active_device_name = active.device_name.clone();
            let active_public_key = active.public_key.clone();
            let previously_trusted = self.trusted.is_paired(&active_sender_id);
            let previous_trusted = self.trusted.clone();
            self.trusted.upsert(trusted_device_from_pairing(
                &active_sender_id,
                &active_device_name,
                &active_public_key,
                now_ms,
            ));
            if !previously_trusted {
                self.prepare_trusted_identity_replacement(&active_sender_id);
            }
            if let Err(error) = self.persist_trusted() {
                self.trusted = previous_trusted;
                return Err(error);
            }
            if let Some(pending) = self.pending_pairing.as_mut() {
                pending.receiver_committed = true;
            }
        }

        let complete = PairingComplete {
            transcript_hash: transcript_hash.to_vec(),
            identity_signature: sign_transcript_phase(
                self.identity.signer(),
                &transcript_hash,
                PAIRING_COMPLETE_PHASE,
            )
            .to_vec(),
        };
        self.send_control_payload(session, ControlPayload::PairingComplete(complete))?;

        if let Some(sender) = self.active_sender.as_mut() {
            sender.video_allowed = true;
        }
        self.pending_pairing = None;
        self.begin_streaming(session);
        Ok(())
    }
}
