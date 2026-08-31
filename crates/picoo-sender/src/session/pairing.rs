use std::path::Path;

use picoo_pairing::{
    pairing_confirm_signature, pairing_transcript_hash, trusted_device_from_pairing,
    TrustedDeviceStore,
};
use picoo_protocol::control::{
    ClientHello, PairingApproval, PairingChallenge, PairingCommit, PairingComplete, PairingConfirm,
    ServerHello, SessionError,
};
use picoo_protocol::ALPN;
use picoo_session::SenderStatus;
use picoo_transport::{PicooTransport, SessionId};
use prost::Message;

use super::SenderSession;
use crate::SenderError;

pub(super) const PAIRING_APPROVAL_MAGIC: u32 = 0x5041_5056;
const PAIRING_COMMIT_MAGIC: u32 = 0x5043_4D54;
pub(super) const PAIRING_COMPLETE_MAGIC: u32 = 0x5043_4D50;
pub(super) const PAIRING_APPROVAL_PHASE: &[u8] = b"pairing-approval-v2";
const PAIRING_COMMIT_PHASE: &[u8] = b"pairing-commit-v2";
pub(super) const PAIRING_COMPLETE_PHASE: &[u8] = b"pairing-complete-v2";

#[derive(Debug, Clone)]
pub(super) struct SenderPairing {
    receiver_id: String,
    display_name: String,
    public_key: Vec<u8>,
    challenge_nonce: Vec<u8>,
    short_code: String,
    confirm_sent: bool,
    trust_committed: bool,
}

#[derive(Debug, Clone)]
pub(super) struct ClientHelloParams {
    sender_id: String,
    device_name: String,
    public_key: Vec<u8>,
    protocol_version: String,
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

    fn pairing_transcript_matches(
        &self,
        session: SessionId,
        nonce: &[u8],
        transcript_hash: &[u8],
        phase: &[u8],
    ) -> bool {
        let Some(pairing) = self.pairing.as_ref() else {
            return false;
        };
        let Some(sender_id) = self.sender_id.as_deref() else {
            return false;
        };
        self.session == Some(session)
            && nonce == pairing.challenge_nonce
            && transcript_hash
                == pairing_transcript_hash(
                    &pairing.challenge_nonce,
                    &pairing.receiver_id,
                    sender_id,
                    phase,
                )
    }

    pub(super) fn handle_pairing_approval(
        &mut self,
        session: SessionId,
        approval: &PairingApproval,
    ) -> bool {
        if approval.magic == PAIRING_APPROVAL_MAGIC
            && self.pairing_transcript_matches(
                session,
                &approval.challenge_nonce,
                &approval.transcript_hash,
                PAIRING_APPROVAL_PHASE,
            )
        {
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
        if complete.magic == PAIRING_COMPLETE_MAGIC
            && self.pairing_transcript_matches(
                session,
                &complete.challenge_nonce,
                &complete.transcript_hash,
                PAIRING_COMPLETE_PHASE,
            )
        {
            self.accept_pairing_complete();
            true
        } else {
            false
        }
    }

    pub(super) fn handle_pairing_challenge(&mut self, challenge: PairingChallenge) -> bool {
        let valid = challenge.challenge_nonce.len() == 32
            && challenge.short_code.len() == 6
            && challenge.short_code.chars().all(|c| c.is_ascii_digit());
        if valid {
            if let Some(pairing) = self.pairing.as_mut() {
                pairing.challenge_nonce = challenge.challenge_nonce;
                pairing.short_code = challenge.short_code;
                pairing.confirm_sent = false;
                pairing.trust_committed = false;
            } else {
                self.pairing = Some(SenderPairing {
                    receiver_id: String::new(),
                    display_name: String::new(),
                    public_key: Vec::new(),
                    challenge_nonce: challenge.challenge_nonce,
                    short_code: challenge.short_code,
                    confirm_sent: false,
                    trust_committed: false,
                });
            }
            self.status = SenderStatus::Pairing;
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
        // Real Hello needs non-empty id + PCP version (empty ver = false positive).
        if hello.receiver_id.is_empty() || hello.protocol_version.is_empty() {
            return;
        }
        // ARCH-PICOO-PROTOCOL-001: reject mismatched PCP version fail-fast.
        if hello.protocol_version != picoo_protocol::ALPN {
            if let Some(session) = self.session.take() {
                self.transport
                    .close(session, picoo_transport::CloseReason::LocalClose);
            }
            self.status = SenderStatus::Disconnected;
            self.pairing = None;
            return;
        }
        if self.trusted.is_paired(&hello.receiver_id) {
            if self
                .trusted
                .verify_paired_key(&hello.receiver_id, &hello.public_key)
                .is_err()
            {
                if let Some(session) = self.session.take() {
                    self.transport
                        .close(session, picoo_transport::CloseReason::LocalClose);
                }
                self.status = SenderStatus::Disconnected;
                self.pairing = None;
                return;
            }
            self.trusted
                .touch_last_connected(&hello.receiver_id, self.now_ms());
            let _ = self.persist_trusted();
        }

        if hello.pairing_required {
            if let Some(pairing) = self.pairing.as_mut() {
                pairing.receiver_id = hello.receiver_id;
                pairing.display_name = hello.display_name;
                pairing.public_key = hello.public_key;
            } else {
                self.pairing = Some(SenderPairing {
                    receiver_id: hello.receiver_id,
                    display_name: hello.display_name,
                    public_key: hello.public_key,
                    challenge_nonce: Vec::new(),
                    short_code: String::new(),
                    confirm_sent: false,
                    trust_committed: false,
                });
            }
            self.status = SenderStatus::Pairing;
        } else {
            if let Some(pairing) = self.pairing.as_mut() {
                pairing.receiver_id = hello.receiver_id;
                pairing.display_name = hello.display_name;
                pairing.public_key = hello.public_key;
            } else {
                self.pairing = Some(SenderPairing {
                    receiver_id: hello.receiver_id,
                    display_name: hello.display_name,
                    public_key: hello.public_key,
                    challenge_nonce: Vec::new(),
                    short_code: String::new(),
                    confirm_sent: false,
                    trust_committed: false,
                });
            }
            self.enter_streaming();
        }
    }

    fn accept_pairing_approval(&mut self) {
        if self.status != SenderStatus::Pairing {
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
            self.trusted.upsert(trusted_device_from_pairing(
                &pairing.receiver_id,
                display_name,
                &pairing.public_key,
                self.now_ms(),
            ));
            if self.persist_trusted().is_err() {
                self.trusted = previous_trusted;
                self.last_session_error = Some("PAIRING_STORE_FAILED".into());
                return;
            }
            if let Some(pairing) = self.pairing.as_mut() {
                pairing.trust_committed = true;
            }
        }

        let Some(active_session) = self.session else {
            self.last_session_error = Some("PAIRING_SESSION_MISSING".into());
            return;
        };
        let Some(sender_id) = self.sender_id.as_deref() else {
            self.last_session_error = Some("PAIRING_SENDER_ID_MISSING".into());
            return;
        };
        let commit = PairingCommit {
            magic: PAIRING_COMMIT_MAGIC,
            challenge_nonce: pairing.challenge_nonce.clone(),
            transcript_hash: pairing_transcript_hash(
                &pairing.challenge_nonce,
                &pairing.receiver_id,
                sender_id,
                PAIRING_COMMIT_PHASE,
            ),
        };
        let mut out = Vec::new();
        if commit.encode(&mut out).is_err()
            || self
                .transport
                .send_control(active_session, bytes::Bytes::from(out))
                .is_err()
        {
            self.last_session_error = Some("PAIRING_COMMIT_SEND_FAILED".into());
            return;
        }
        self.last_session_error = None;
    }

    fn accept_pairing_complete(&mut self) {
        if self.status != SenderStatus::Pairing {
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

    pub fn send_client_hello(
        &mut self,
        sender_id: &str,
        device_name: &str,
        public_key: &[u8],
    ) -> Result<(), SenderError> {
        self.send_client_hello_with_version(sender_id, device_name, public_key, ALPN)
    }

    /// Emit ClientHello with an explicit protocol_version (protocol fail-fast tests).
    pub fn send_client_hello_with_version(
        &mut self,
        sender_id: &str,
        device_name: &str,
        public_key: &[u8],
        protocol_version: &str,
    ) -> Result<(), SenderError> {
        let connection_pending = matches!(
            self.status,
            SenderStatus::Connecting | SenderStatus::Reconnecting
        );
        if self.session.is_none() && !connection_pending {
            return Err(SenderError::NotConnected);
        }

        self.last_session_error = None;
        self.sender_id = Some(sender_id.into());
        let params = ClientHelloParams {
            sender_id: sender_id.to_string(),
            device_name: device_name.to_string(),
            public_key: public_key.to_vec(),
            protocol_version: protocol_version.to_string(),
        };
        self.hello_params = Some(params.clone());

        // QUIC connect is asynchronous on mobile. Treat ClientHello as the desired
        // first control message and let `on_connected` emit it once a session exists.
        // This preserves the Android call order: connect() -> sendClientHello().
        if self.session.is_none() {
            return Ok(());
        }

        self.emit_client_hello(&params)?;
        self.drain_events();
        Ok(())
    }

    pub(super) fn emit_client_hello(
        &mut self,
        params: &ClientHelloParams,
    ) -> Result<(), SenderError> {
        let session = self.session.ok_or(SenderError::NotConnected)?;
        let hello = ClientHello {
            sender_id: params.sender_id.clone(),
            device_name: params.device_name.clone(),
            protocol_version: params.protocol_version.clone(),
            public_key: params.public_key.clone(),
        };
        let mut buf = Vec::new();
        hello
            .encode(&mut buf)
            .map_err(|e| SenderError::Protocol(e.to_string()))?;
        self.transport
            .send_control(session, bytes::Bytes::from(buf))
            .map_err(SenderError::Transport)?;
        Ok(())
    }

    pub fn send_pairing_confirm(&mut self, receiver_id: &str) -> Result<(), SenderError> {
        let session = self.session.ok_or(SenderError::NotConnected)?;
        let pairing = self
            .pairing
            .as_ref()
            .ok_or_else(|| SenderError::Protocol("no pairing challenge".into()))?;
        let sender_id = self
            .sender_id
            .as_deref()
            .ok_or_else(|| SenderError::Protocol("missing sender id".into()))?;
        if pairing.receiver_id != receiver_id {
            return Err(SenderError::Protocol(
                "pairing receiver id does not match ServerHello".into(),
            ));
        }

        let confirm = PairingConfirm {
            confirm_signature: pairing_confirm_signature(
                &pairing.challenge_nonce,
                receiver_id,
                sender_id,
            ),
        };
        let mut buf = Vec::new();
        confirm
            .encode(&mut buf)
            .map_err(|e| SenderError::Protocol(e.to_string()))?;
        self.transport
            .send_control(session, bytes::Bytes::from(buf))
            .map_err(SenderError::Transport)?;
        if let Some(pairing) = self.pairing.as_mut() {
            pairing.confirm_sent = true;
        }
        self.last_session_error = None;
        // The receiver may still be waiting for its local user. Trust and media start only
        // after its authenticated PairingComplete acknowledgement (REQ-PICOO-PAIRING-001).
        Ok(())
    }
}
