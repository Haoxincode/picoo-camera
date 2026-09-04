use std::path::Path;

use picoo_pairing::{
    derive_device_id, random_challenge_nonce, sign_transcript_phase, trusted_device_from_pairing,
    verify_transcript_phase, PairingTranscript, TrustedDeviceStore,
};
use picoo_protocol::control::{
    control_envelope::Payload as ControlPayload, ClientHello, PairingApproval, PairingCommit,
    PairingComplete, PairingConfirm, ServerHello, SessionError,
};
use picoo_session::{ConnectionState, SenderStatus, StreamState, TrustState};
use picoo_transport::{PicooTransport, SessionId};

use super::{SenderEvent, SenderSession};
use crate::SenderError;

pub(super) const SERVER_HELLO_PHASE: &[u8] = b"server-hello";
pub(super) const SENDER_CONFIRM_PHASE: &[u8] = b"sender-confirm";
pub(super) const PAIRING_APPROVAL_PHASE: &[u8] = b"receiver-approval";
const PAIRING_COMMIT_PHASE: &[u8] = b"sender-commit";
pub(super) const PAIRING_COMPLETE_PHASE: &[u8] = b"receiver-complete";

#[derive(Debug, Clone)]
pub(super) struct SenderPairing {
    receiver_id: String,
    display_name: String,
    public_key: Vec<u8>,
    transcript_hash: [u8; 32],
    short_code: String,
    pairing_required: bool,
    confirm_sent: bool,
    trust_committed: bool,
}

impl<T: PicooTransport> SenderSession<T> {
    pub fn with_trusted_store(mut self, path: impl AsRef<Path>) -> Result<Self, SenderError> {
        let path = path.as_ref().to_path_buf();
        self.trusted = TrustedDeviceStore::load_from_path(&path)?;
        self.trusted_store_path = Some(path);
        Ok(self)
    }

    pub fn attach_trusted_store(&mut self, path: impl AsRef<Path>) -> Result<(), SenderError> {
        let path = path.as_ref().to_path_buf();
        self.trusted = TrustedDeviceStore::load_from_path(&path)?;
        self.trusted_store_path = Some(path);
        Ok(())
    }

    pub fn trusted_devices(&self) -> &TrustedDeviceStore {
        &self.trusted
    }

    pub fn trusted_devices_mut(&mut self) -> &mut TrustedDeviceStore {
        &mut self.trusted
    }

    pub fn remove_trusted_device(&mut self, device_id: &str) -> Result<bool, SenderError> {
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

    pub fn connected_receiver_id(&self) -> Option<&str> {
        self.pairing
            .as_ref()
            .and_then(|p| (!p.receiver_id.is_empty()).then_some(p.receiver_id.as_str()))
    }

    /// Display name from ServerHello (empty until hello arrives).
    pub fn connected_receiver_display_name(&self) -> Option<&str> {
        self.pairing
            .as_ref()
            .and_then(|p| (!p.display_name.is_empty()).then_some(p.display_name.as_str()))
    }

    pub(super) fn receiver_is_authenticated(&self) -> bool {
        let Some(pairing) = self.pairing.as_ref() else {
            return false;
        };
        !pairing.receiver_id.is_empty()
            && self
                .trusted
                .verify_paired_key(&pairing.receiver_id, &pairing.public_key)
                .is_ok()
    }

    fn persist_trusted(&self) -> Result<(), SenderError> {
        if let Some(path) = &self.trusted_store_path {
            self.trusted.save_to_path(path)?;
        }
        Ok(())
    }

    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    fn verify_receiver_phase(
        &self,
        session: SessionId,
        transcript_hash: &[u8],
        signature: &[u8],
        phase: &[u8],
    ) -> bool {
        let Some(pairing) = self.pairing.as_ref() else {
            return false;
        };
        self.active_session() == Some(session)
            && transcript_hash == pairing.transcript_hash
            && verify_transcript_phase(
                &pairing.public_key,
                &pairing.transcript_hash,
                phase,
                signature,
            )
            .is_ok()
    }

    pub(super) fn handle_pairing_approval(
        &mut self,
        session: SessionId,
        approval: &PairingApproval,
    ) -> bool {
        if self.verify_receiver_phase(
            session,
            &approval.transcript_hash,
            &approval.identity_signature,
            PAIRING_APPROVAL_PHASE,
        ) {
            self.accept_pairing_approval();
            true
        } else {
            false
        }
    }

    pub(super) fn handle_pairing_complete(
        &mut self,
        session: SessionId,
        complete: &PairingComplete,
    ) -> bool {
        if self.verify_receiver_phase(
            session,
            &complete.transcript_hash,
            &complete.identity_signature,
            PAIRING_COMPLETE_PHASE,
        ) {
            self.accept_pairing_complete();
            true
        } else {
            false
        }
    }

    pub(super) fn handle_session_error(&mut self, err: SessionError) -> bool {
        if matches!(
            err.code.as_str(),
            "UNPAIRED" | "PUBLIC_KEY_CHANGED" | "PAIRING_REJECTED"
        ) {
            self.last_session_error = Some(err.code);
            true
        } else {
            false
        }
    }

    pub(super) fn on_server_hello(&mut self, hello: ServerHello) {
        let Some(session) = self.active_session() else {
            self.reject_authentication("PAIRING_SESSION_MISSING");
            return;
        };
        let Some(sender_nonce) = self.sender_nonce else {
            self.reject_authentication("PAIRING_SENDER_NONCE_MISSING");
            return;
        };
        if hello.receiver_id.is_empty()
            || derive_device_id(&hello.public_key) != hello.receiver_id
            || hello.receiver_nonce.len() != 32
        {
            self.reject_authentication("INVALID_RECEIVER_IDENTITY");
            return;
        }
        let Ok(channel_binding) = self.transport.channel_binding(session) else {
            self.reject_authentication("CHANNEL_BINDING_UNAVAILABLE");
            return;
        };
        let transcript = PairingTranscript {
            sender_id: self.identity.device_id(),
            sender_public_key: self.identity.public_key(),
            sender_nonce: &sender_nonce,
            receiver_id: &hello.receiver_id,
            receiver_public_key: &hello.public_key,
            receiver_nonce: &hello.receiver_nonce,
            channel_binding: &channel_binding,
            connection_generation: session.0,
        };
        let Ok(transcript_hash) = transcript.hash() else {
            self.reject_authentication("INVALID_PAIRING_TRANSCRIPT");
            return;
        };
        let Ok(short_code) = transcript.short_code() else {
            self.reject_authentication("INVALID_PAIRING_TRANSCRIPT");
            return;
        };
        if hello.transcript_hash != transcript_hash
            || verify_transcript_phase(
                &hello.public_key,
                &transcript_hash,
                SERVER_HELLO_PHASE,
                &hello.identity_signature,
            )
            .is_err()
        {
            self.reject_authentication("INVALID_RECEIVER_PROOF");
            return;
        }

        let receiver_is_trusted = self.trusted.is_paired(&hello.receiver_id);
        if receiver_is_trusted
            && self
                .trusted
                .verify_paired_key(&hello.receiver_id, &hello.public_key)
                .is_err()
        {
            self.reject_authentication("PUBLIC_KEY_CHANGED");
            return;
        }

        if !hello.pairing_required && !receiver_is_trusted {
            self.reject_authentication("UNTRUSTED_PAIRING_BYPASS");
            return;
        }

        self.pairing = Some(SenderPairing {
            receiver_id: hello.receiver_id,
            display_name: hello.display_name,
            public_key: hello.public_key,
            transcript_hash,
            short_code: if hello.pairing_required {
                short_code
            } else {
                String::new()
            },
            pairing_required: hello.pairing_required,
            confirm_sent: false,
            trust_committed: false,
        });

        if hello.pairing_required {
            self.lifecycle.runtime.set_trust(TrustState::Pairing);
            self.lifecycle.runtime.set_stream(StreamState::Negotiating);
        } else {
            self.lifecycle.runtime.set_trust(TrustState::Unknown);
            self.lifecycle.runtime.set_stream(StreamState::Negotiating);
            if self.send_identity_confirm().is_err() {
                self.reject_authentication("SENDER_PROOF_SEND_FAILED");
            }
        }
    }

    pub(super) fn reject_authentication(&mut self, code: &str) {
        self.last_session_error = Some(code.into());
        if let Some(session) = self.active_session() {
            let _ = self.apply_sender_event(SenderEvent::AuthenticationRejected {
                generation: session.0,
            });
        } else {
            let _ = self.apply_sender_event(SenderEvent::UserDisconnect {
                domain_resources_active: true,
            });
        }
    }

    fn accept_pairing_approval(&mut self) {
        if !matches!(
            self.status(),
            SenderStatus::Pairing | SenderStatus::Negotiating
        ) {
            return;
        }
        let Some(pairing) = self.pairing.clone() else {
            self.last_session_error = Some("PAIRING_STATE_MISSING".into());
            return;
        };
        if pairing.receiver_id.is_empty() {
            self.last_session_error = Some("PAIRING_RECEIVER_ID_MISSING".into());
            return;
        }
        if !pairing.confirm_sent {
            self.last_session_error = Some("PAIRING_LOCAL_CONFIRM_MISSING".into());
            return;
        }

        if !pairing.trust_committed {
            let display_name = if pairing.display_name.is_empty() {
                pairing.receiver_id.as_str()
            } else {
                pairing.display_name.as_str()
            };
            let previous_trusted = self.trusted.clone();
            if self.trusted.is_paired(&pairing.receiver_id) {
                self.trusted
                    .touch_last_connected(&pairing.receiver_id, self.now_ms());
            } else {
                self.trusted.upsert(trusted_device_from_pairing(
                    &pairing.receiver_id,
                    display_name,
                    &pairing.public_key,
                    self.now_ms(),
                ));
            }
            if self.persist_trusted().is_err() {
                self.trusted = previous_trusted;
                self.last_session_error = Some("PAIRING_STORE_FAILED".into());
                return;
            }
            if let Some(pairing) = self.pairing.as_mut() {
                pairing.trust_committed = true;
            }
        }

        let Some(active_session) = self.active_session() else {
            self.last_session_error = Some("PAIRING_SESSION_MISSING".into());
            return;
        };
        let commit = PairingCommit {
            transcript_hash: pairing.transcript_hash.to_vec(),
            identity_signature: sign_transcript_phase(
                &self.identity,
                &pairing.transcript_hash,
                PAIRING_COMMIT_PHASE,
            )
            .to_vec(),
        };
        if self
            .send_control_payload(active_session, ControlPayload::PairingCommit(commit))
            .is_err()
        {
            self.last_session_error = Some("PAIRING_COMMIT_SEND_FAILED".into());
            return;
        }
        self.last_session_error = None;
    }

    fn accept_pairing_complete(&mut self) {
        if !matches!(
            self.status(),
            SenderStatus::Pairing | SenderStatus::Negotiating
        ) {
            return;
        }
        let Some(pairing) = self.pairing.as_ref() else {
            return;
        };
        if !pairing.confirm_sent || !pairing.trust_committed {
            self.last_session_error = Some("PAIRING_COMMIT_MISSING".into());
            return;
        }
        if self
            .trusted
            .verify_paired_key(&pairing.receiver_id, &pairing.public_key)
            .is_err()
        {
            self.last_session_error = Some("PAIRING_STORE_MISMATCH".into());
            return;
        }
        self.last_session_error = None;
        self.enter_streaming();
    }

    pub fn pairing_short_code(&self) -> Option<&str> {
        self.pairing
            .as_ref()
            .and_then(|p| (!p.short_code.is_empty()).then_some(p.short_code.as_str()))
    }

    pub fn send_client_hello(&mut self) -> Result<(), SenderError> {
        let connection_pending = matches!(
            self.lifecycle.runtime.connection(),
            ConnectionState::Connecting | ConnectionState::Reconnecting { .. }
        );
        if self.active_session().is_none() && !connection_pending {
            return Err(SenderError::NotConnected);
        }

        self.last_session_error = None;
        self.hello_requested = true;

        // QUIC connect is asynchronous on mobile. Treat ClientHello as the desired
        // first control message and let `on_connected` emit it once a session exists.
        // This preserves the Android call order: connect() -> sendClientHello().
        if self.active_session().is_none() {
            return Ok(());
        }

        self.emit_client_hello()?;
        self.lifecycle.runtime.set_stream(StreamState::Negotiating);
        self.drain_events();
        Ok(())
    }

    pub(super) fn emit_client_hello(&mut self) -> Result<(), SenderError> {
        let session = self.active_session().ok_or(SenderError::NotConnected)?;
        let sender_nonce =
            random_challenge_nonce().map_err(|error| SenderError::Protocol(error.to_string()))?;
        self.sender_nonce = Some(sender_nonce);
        self.pairing = None;
        let hello = ClientHello {
            sender_id: self.identity.device_id().to_owned(),
            device_name: self.identity.device_name().to_owned(),
            public_key: self.identity.public_key().to_vec(),
            sender_nonce: sender_nonce.to_vec(),
        };
        self.send_control_payload(session, ControlPayload::ClientHello(hello))?;
        Ok(())
    }

    pub fn send_pairing_confirm(&mut self, receiver_id: &str) -> Result<(), SenderError> {
        let pairing = self
            .pairing
            .as_ref()
            .ok_or_else(|| SenderError::Protocol("no pairing challenge".into()))?;
        if pairing.receiver_id != receiver_id {
            return Err(SenderError::Protocol(
                "pairing receiver id does not match ServerHello".into(),
            ));
        }
        if !pairing.pairing_required {
            return Err(SenderError::Protocol(
                "trusted reconnect does not require user confirmation".into(),
            ));
        }
        self.send_identity_confirm()
    }

    fn send_identity_confirm(&mut self) -> Result<(), SenderError> {
        let session = self.active_session().ok_or(SenderError::NotConnected)?;
        let pairing = self
            .pairing
            .as_ref()
            .ok_or_else(|| SenderError::Protocol("no authentication challenge".into()))?;
        let confirm = PairingConfirm {
            transcript_hash: pairing.transcript_hash.to_vec(),
            identity_signature: sign_transcript_phase(
                &self.identity,
                &pairing.transcript_hash,
                SENDER_CONFIRM_PHASE,
            )
            .to_vec(),
        };
        self.send_control_payload(session, ControlPayload::PairingConfirm(confirm))?;
        if let Some(pairing) = self.pairing.as_mut() {
            pairing.confirm_sent = true;
        }
        self.last_session_error = None;
        // The receiver may still be waiting for its local user. Trust and media start only
        // after its authenticated PairingComplete acknowledgement (REQ-PICOO-PAIRING-001).
        Ok(())
    }
}
