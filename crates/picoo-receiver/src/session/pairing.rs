//! Pairing handshake: ClientHello, short-code confirm, commit, trusted store.
//!
//! REQ-PICOO-PAIRING-*

use std::path::Path;
use std::time::{Duration, Instant};

use picoo_pairing::{
    new_pairing_challenge, pairing_transcript_hash, random_challenge_nonce,
    trusted_device_from_pairing, verify_pairing_confirm, PairingHandshakeError, TrustedDeviceStore,
};
use picoo_protocol::control::{
    control_envelope::Payload as ControlPayload, ClientHello, PairingApproval,
    PairingChallenge as PairingChallengeMsg, PairingCommit, PairingComplete, PairingConfirm,
    ServerHello, SessionError,
};
use picoo_session::ReceiverStatus;
use picoo_transport::{CloseReason, SessionId};

use super::{ReceiverSession, TrustedIdentityCandidate, TrustedIdentityReplacement};
use crate::{ReceiverError, PAIRING_CHALLENGE_TTL};

pub(in crate::session) struct ActiveSender {
    pub(in crate::session) sender_id: String,
    pub(in crate::session) device_name: String,
    pub(in crate::session) public_key: Vec<u8>,
    pub(in crate::session) video_allowed: bool,
}

pub(in crate::session) struct PendingPairing {
    pub(in crate::session) session: SessionId,
    pub(in crate::session) challenge_nonce: Vec<u8>,
    pub(in crate::session) short_code: String,
    pub(in crate::session) local_confirmed: bool,
    pub(in crate::session) remote_confirmed: bool,
    pub(in crate::session) sender_committed: bool,
    pub(in crate::session) receiver_committed: bool,
    /// PUC-001 / AC-M-PAIR-02: challenge valid for 60s (wall clock).
    pub(in crate::session) expires_at: Instant,
}

pub(in crate::session) const PAIRING_APPROVAL_PHASE: &[u8] = b"pairing-approval-v2";
pub(in crate::session) const PAIRING_COMMIT_PHASE: &[u8] = b"pairing-commit-v2";
pub(in crate::session) const PAIRING_COMPLETE_PHASE: &[u8] = b"pairing-complete-v2";

impl ReceiverSession {
    pub fn with_trusted_store(mut self, path: impl AsRef<Path>) -> Result<Self, ReceiverError> {
        let path = path.as_ref().to_path_buf();
        self.trusted = TrustedDeviceStore::load_from_path(&path)?;
        self.trusted_store_path = Some(path);
        Ok(self)
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
        self.active_sender
            .as_ref()
            .is_some_and(|sender| !sender.video_allowed)
    }

    pub fn pairing_short_code(&self) -> Option<&str> {
        self.pending_pairing.as_ref().map(|p| p.short_code.as_str())
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
        self.pending_pairing = None;
        self.active_sender = None;
        self.transport.close(
            session,
            CloseReason::Error("pairing challenge expired".into()),
        );
        self.status = if self.bind_addr().is_some() {
            ReceiverStatus::Discovering
        } else {
            ReceiverStatus::Disconnected
        };
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
        self.transport.close(session, CloseReason::LocalClose);
        self.active_sender = None;
        self.pending_pairing = None;
        self.status = if self.bind_addr().is_some() {
            ReceiverStatus::Discovering
        } else {
            ReceiverStatus::Disconnected
        };
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
            self.transport.close(session, CloseReason::LocalClose);
            self.active_sender = None;
            self.pending_pairing = None;
            self.status = if self.bind_addr().is_some() {
                ReceiverStatus::Discovering
            } else {
                ReceiverStatus::Disconnected
            };
            return Ok(());
        }

        let paired = self
            .trusted
            .verify_paired_key(&hello.sender_id, &hello.public_key)
            .is_ok();
        let auto_accept = paired && self.auto_accept_paired;

        let server_hello = ServerHello {
            receiver_id: self.identity.receiver_id.clone(),
            display_name: self.identity.display_name.clone(),
            public_key: self.identity.public_key.clone(),
            pairing_required: !auto_accept,
        };
        self.send_control_payload(session, ControlPayload::ServerHello(server_hello))?;

        if auto_accept {
            self.trusted
                .touch_last_connected(&hello.sender_id, self.now_ms());
            self.persist_trusted()?;
            self.active_sender = Some(ActiveSender {
                sender_id: hello.sender_id,
                device_name: hello.device_name,
                public_key: hello.public_key,
                video_allowed: true,
            });
            return self.begin_streaming(session);
        }

        let nonce = random_challenge_nonce();
        let challenge = new_pairing_challenge(&nonce, &self.identity.receiver_id, &hello.sender_id);
        let challenge_msg = PairingChallengeMsg {
            short_code: challenge.short_code.clone(),
            challenge_nonce: challenge.challenge_nonce,
        };
        self.send_control_payload(session, ControlPayload::PairingChallenge(challenge_msg))?;

        self.pending_pairing = Some(PendingPairing {
            session,
            challenge_nonce: nonce,
            short_code: challenge.short_code,
            local_confirmed: false,
            remote_confirmed: false,
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
        self.status = ReceiverStatus::Pairing;
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
            if matches!(self.status, ReceiverStatus::Pairing) {
                self.status = ReceiverStatus::Connecting;
            }
            return Err(ReceiverError::Protocol("pairing challenge expired".into()));
        }

        if session != pending.session {
            return Err(ReceiverError::Protocol("pairing session mismatch".into()));
        }

        let sender_id = self
            .active_sender
            .as_ref()
            .map(|s| s.sender_id.as_str())
            .unwrap_or("");

        verify_pairing_confirm(
            &pending.challenge_nonce,
            &self.identity.receiver_id,
            sender_id,
            &confirm.confirm_signature,
        )
        .map_err(|e| match e {
            PairingHandshakeError::InvalidSignature => {
                ReceiverError::Protocol("invalid pairing signature".into())
            }
        })?;

        if let Some(pending) = self.pending_pairing.as_mut() {
            pending.remote_confirmed = true;
        }
        self.advance_pairing()
    }

    pub(crate) fn pairing_transcript_matches(
        &self,
        session: SessionId,
        nonce: &[u8],
        transcript_hash: &[u8],
        phase: &[u8],
    ) -> bool {
        let Some(pending) = self.pending_pairing.as_ref() else {
            return false;
        };
        let Some(active) = self.active_sender.as_ref() else {
            return false;
        };
        session == pending.session
            && nonce == pending.challenge_nonce
            && transcript_hash
                == pairing_transcript_hash(
                    &pending.challenge_nonce,
                    &self.identity.receiver_id,
                    &active.sender_id,
                    phase,
                )
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
        if self.pairing_transcript_matches(
            session,
            &commit.challenge_nonce,
            &commit.transcript_hash,
            PAIRING_COMMIT_PHASE,
        ) {
            self.finish_pairing_commit()
        } else {
            Ok(())
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
        let challenge_nonce = pending.challenge_nonce.clone();
        let sender_committed = pending.sender_committed;
        let receiver_committed = pending.receiver_committed;
        let sender_id = self
            .active_sender
            .as_ref()
            .map(|active| active.sender_id.clone())
            .unwrap_or_default();

        if !sender_committed {
            let approval = PairingApproval {
                challenge_nonce: challenge_nonce.clone(),
                transcript_hash: pairing_transcript_hash(
                    &challenge_nonce,
                    &self.identity.receiver_id,
                    &sender_id,
                    PAIRING_APPROVAL_PHASE,
                ),
            };
            return self.send_control_payload(session, ControlPayload::PairingApproval(approval));
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
            challenge_nonce: challenge_nonce.clone(),
            transcript_hash: pairing_transcript_hash(
                &challenge_nonce,
                &self.identity.receiver_id,
                &sender_id,
                PAIRING_COMPLETE_PHASE,
            ),
        };
        self.send_control_payload(session, ControlPayload::PairingComplete(complete))?;

        if let Some(sender) = self.active_sender.as_mut() {
            sender.video_allowed = true;
        }
        self.pending_pairing = None;
        self.begin_streaming(session)
    }
}
